use embassy_futures::select::{select, Either};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Duration, Instant, Timer};

mod runtime;
mod state;

use crate::drivers::inkplate::TouchInitStatus;
use crate::firmware::{
    input::gpio36::{Gpio36Classifier, Gpio36Mode},
    types::{Gpio36InputPin, InkplateTouchDriver, TouchStatus},
};

use super::request_touch_pipeline_reset;
use crate::firmware::touch::config::{GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED, TOUCH_INIT_RETRY_MS};
use runtime::{
    handle_fault, probe_asserted_line, publish_action, publish_touch_status, read_and_publish,
    wait_for_assertion_or_recovery, AcquisitionWake, ProbeResult,
};
use state::ContactSamplingState;

#[derive(Clone, Copy)]
enum TouchAcquisitionCommand {
    Suspend,
    Resume { reset_pipeline: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TouchAcquisitionState {
    Suspended,
    Running,
}

static TOUCH_ACQUISITION_COMMANDS: Channel<CriticalSectionRawMutex, TouchAcquisitionCommand, 2> =
    Channel::new();
static TOUCH_ACQUISITION_STATE: Signal<CriticalSectionRawMutex, TouchAcquisitionState> =
    Signal::new();

pub(crate) async fn suspend_touch_acquisition() {
    TOUCH_ACQUISITION_COMMANDS
        .send(TouchAcquisitionCommand::Suspend)
        .await;
    while TOUCH_ACQUISITION_STATE.wait().await != TouchAcquisitionState::Suspended {}
}

pub(crate) async fn resume_touch_acquisition(reset_pipeline: bool) {
    TOUCH_ACQUISITION_COMMANDS
        .send(TouchAcquisitionCommand::Resume { reset_pipeline })
        .await;
    while TOUCH_ACQUISITION_STATE.wait().await != TouchAcquisitionState::Running {}
}

/// Queues a resume without waiting for the first post-resume I2C sample.
///
/// The display task uses this after panel power-up so the touch core can sample
/// concurrently with row scanning. A later acknowledged suspend closes the
/// window before panel power-down touches the shared I2C bus again.
pub(crate) async fn request_touch_acquisition_resume(reset_pipeline: bool) {
    TOUCH_ACQUISITION_COMMANDS
        .send(TouchAcquisitionCommand::Resume { reset_pipeline })
        .await;
}

#[embassy_executor::task]
pub(crate) async fn touch_acquisition_task(
    mut touch: InkplateTouchDriver,
    mut gpio36: Gpio36InputPin,
    initial_touch_resolution: Option<(u16, u16)>,
) {
    let mode = if GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED {
        Gpio36Mode::ButtonOnly
    } else {
        Gpio36Mode::SharedWithTouch
    };
    let mut classifier = Gpio36Classifier::new();
    let mut touch_ready = initial_touch_resolution.is_some();
    let mut retry_at = if touch_ready {
        Instant::now()
    } else {
        Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS)
    };
    let mut stack_sample_at = Instant::now();
    let mut contact_sampling = ContactSamplingState::new();

    if matches!(mode, Gpio36Mode::ButtonOnly) {
        let _ = touch.shutdown().await;
        publish_touch_status(TouchStatus::Fault).await;
    } else if let Some((x_res, y_res)) = initial_touch_resolution {
        esp_println::println!(
            "touch: ready phase=bootstrap x_res={} y_res={}",
            x_res,
            y_res
        );
        request_touch_pipeline_reset();
        publish_touch_status(TouchStatus::Ready { x_res, y_res }).await;
    }

    loop {
        let now = Instant::now();
        if matches!(mode, Gpio36Mode::SharedWithTouch) && !touch_ready && now >= retry_at {
            publish_touch_status(TouchStatus::Initializing).await;
            match touch.init_with_status().await {
                Ok(TouchInitStatus::Ready { x_res, y_res }) => {
                    esp_println::println!(
                        "touch: ready phase=acquisition x_res={} y_res={}",
                        x_res,
                        y_res
                    );
                    touch_ready = true;
                    request_touch_pipeline_reset();
                    publish_touch_status(TouchStatus::Ready { x_res, y_res }).await;
                }
                status => {
                    let ext = touch.probe_external().await;
                    let controller = touch.probe_controller().await;
                    esp_println::println!(
                        "touch: init_failed phase=acquisition status={:?} probe_ext={} probe_touch={}",
                        status,
                        ext,
                        controller
                    );
                    let _ = touch.shutdown().await;
                    retry_at = Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS);
                    publish_touch_status(TouchStatus::Fault).await;
                }
            }
        }

        if matches!(mode, Gpio36Mode::SharedWithTouch) && !touch_ready {
            let wait_ms = retry_at
                .as_millis()
                .saturating_sub(Instant::now().as_millis())
                .max(1);
            match select(
                TOUCH_ACQUISITION_COMMANDS.receive(),
                Timer::after_millis(wait_ms),
            )
            .await
            {
                Either::First(command) => {
                    handle_control_command(
                        command,
                        &mut touch,
                        &mut touch_ready,
                        &mut retry_at,
                        &mut classifier,
                        &mut contact_sampling,
                    )
                    .await;
                }
                Either::Second(_) => {}
            }
            continue;
        }

        // Level waits close the check-to-arm race: if GPIO36 asserted just
        // before the waiter is installed, the low level still wakes us.
        match wait_for_assertion_or_recovery(&mut gpio36, contact_sampling.poll_interval_ms()).await
        {
            AcquisitionWake::Command(command) => {
                handle_control_command(
                    command,
                    &mut touch,
                    &mut touch_ready,
                    &mut retry_at,
                    &mut classifier,
                    &mut contact_sampling,
                )
                .await;
                continue;
            }
            AcquisitionWake::RecoveryDue => {
                if touch_ready
                    && read_and_publish(
                        &mut touch,
                        Instant::now().as_millis(),
                        &mut classifier,
                        &mut contact_sampling,
                    )
                    .await
                    .is_err()
                {
                    handle_fault(&mut touch, &mut touch_ready, &mut retry_at).await;
                }
                continue;
            }
            AcquisitionWake::Asserted => {}
        }

        let edge_ms = Instant::now().as_millis();
        esp_println::println!("input: gpio36 raw=low");
        publish_action(classifier.on_asserted(edge_ms, mode)).await;

        if touch_ready {
            match probe_asserted_line(&mut touch, &gpio36, &mut classifier, &mut contact_sampling)
                .await
            {
                ProbeResult::Complete => {}
                ProbeResult::Command(command) => {
                    handle_control_command(
                        command,
                        &mut touch,
                        &mut touch_ready,
                        &mut retry_at,
                        &mut classifier,
                        &mut contact_sampling,
                    )
                    .await;
                    continue;
                }
                ProbeResult::Fault => {
                    handle_fault(&mut touch, &mut touch_ready, &mut retry_at).await;
                    continue;
                }
            }
        }

        match select(TOUCH_ACQUISITION_COMMANDS.receive(), gpio36.wait_for_high()).await {
            Either::First(command) => {
                handle_control_command(
                    command,
                    &mut touch,
                    &mut touch_ready,
                    &mut retry_at,
                    &mut classifier,
                    &mut contact_sampling,
                )
                .await;
                continue;
            }
            Either::Second(_) => {}
        }
        esp_println::println!("input: gpio36 raw=high");
        publish_action(classifier.on_released()).await;

        if Instant::now() >= stack_sample_at {
            crate::firmware::telemetry::record_touch_core_stack_headroom();
            stack_sample_at = Instant::now() + Duration::from_secs(1);
        }
    }
}

async fn handle_control_command(
    command: TouchAcquisitionCommand,
    touch: &mut InkplateTouchDriver,
    touch_ready: &mut bool,
    retry_at: &mut Instant,
    classifier: &mut Gpio36Classifier,
    contact_sampling: &mut ContactSamplingState,
) {
    match command {
        TouchAcquisitionCommand::Resume { .. } => return,
        TouchAcquisitionCommand::Suspend => {}
    }

    classifier.cancel_pending();
    TOUCH_ACQUISITION_STATE.signal(TouchAcquisitionState::Suspended);
    let reset_pipeline = loop {
        let command = TOUCH_ACQUISITION_COMMANDS.receive().await;
        match command {
            TouchAcquisitionCommand::Suspend => {
                TOUCH_ACQUISITION_STATE.signal(TouchAcquisitionState::Suspended);
            }
            TouchAcquisitionCommand::Resume { reset_pipeline } => {
                break reset_pipeline;
            }
        }
    };

    if *touch_ready {
        if reset_pipeline {
            if touch.read_sample(0).await.is_err() {
                handle_fault(touch, touch_ready, retry_at).await;
            }
            request_touch_pipeline_reset();
        } else if read_and_publish(
            touch,
            Instant::now().as_millis(),
            classifier,
            contact_sampling,
        )
        .await
        .is_err()
        {
            handle_fault(touch, touch_ready, retry_at).await;
        }
    } else if reset_pipeline {
        request_touch_pipeline_reset();
    }

    TOUCH_ACQUISITION_STATE.signal(TouchAcquisitionState::Running);
}
