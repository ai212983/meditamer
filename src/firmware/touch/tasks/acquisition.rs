use core::sync::atomic::{AtomicBool, Ordering};

use embassy_futures::{
    select::{select, Either},
    yield_now,
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Duration, Instant, Timer};
use esp_sync::raw::{RawLock, SingleCoreInterruptLock};

mod runtime;
mod state;

use crate::firmware::{
    input::gpio36::{Gpio36Classifier, Gpio36Mode},
    types::{Gpio36InputPin, InkplateTouchDriver, TouchSampleFrame, TouchStatus},
};
use crate::platform::inkplate::{TouchInitStatus, TouchPoint, TouchSample};

use super::{push_touch_input_sample, request_touch_pipeline_reset, reset_touch_pipeline};
use crate::firmware::touch::{
    config::{GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED, TOUCH_INIT_RETRY_MS},
    replay::PIPELINE_REPLAY_TAP,
    scheduling,
};
use runtime::{
    handle_fault, probe_asserted_line, publish_action, publish_touch_status, read_and_publish,
    wait_for_assertion_or_recovery, AcquisitionWake, ProbeResult,
};
use state::ContactSamplingState;

const POWER_DOWN_DURING_PANEL_DIAGNOSTIC: bool =
    option_env!("MEDITAMER_TOUCH_POWER_DOWN_DURING_PANEL").is_some();
const HARD_PARK_DURING_PANEL_DIAGNOSTIC: bool =
    option_env!("MEDITAMER_TOUCH_CORE_HARD_PARK").is_some();

static TOUCH_CORE_HARD_PARKED: AtomicBool = AtomicBool::new(false);
static TOUCH_CORE_HARD_PARK_RESUME: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
enum TouchAcquisitionCommand {
    Suspend,
    Resume { reset_pipeline: bool },
    ReplayPipelineTap,
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
    if HARD_PARK_DURING_PANEL_DIAGNOSTIC {
        while !TOUCH_CORE_HARD_PARKED.load(Ordering::Acquire) {
            yield_now().await;
        }
    }
}

pub(crate) async fn resume_touch_acquisition(reset_pipeline: bool) {
    TOUCH_ACQUISITION_COMMANDS
        .send(TouchAcquisitionCommand::Resume { reset_pipeline })
        .await;
    if HARD_PARK_DURING_PANEL_DIAGNOSTIC {
        TOUCH_CORE_HARD_PARK_RESUME.store(true, Ordering::Release);
    }
    while TOUCH_ACQUISITION_STATE.wait().await != TouchAcquisitionState::Running {}
}

/// Queue a resume without waiting for the first post-resume controller sample.
/// The display task uses this to sample releases concurrently with the GPIO
/// waveform, then closes the window before panel shutdown needs shared I2C.
pub(crate) async fn request_touch_acquisition_resume(reset_pipeline: bool) {
    TOUCH_ACQUISITION_COMMANDS
        .send(TouchAcquisitionCommand::Resume { reset_pipeline })
        .await;
}

pub(crate) fn try_request_touch_acquisition_resume(reset_pipeline: bool) -> bool {
    TOUCH_ACQUISITION_COMMANDS
        .try_send(TouchAcquisitionCommand::Resume { reset_pipeline })
        .is_ok()
}

pub(crate) async fn request_touch_pipeline_replay_probe() {
    TOUCH_ACQUISITION_COMMANDS
        .send(TouchAcquisitionCommand::ReplayPipelineTap)
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
        console::println!(
            "touch: ready phase=bootstrap x_res={} y_res={}",
            x_res,
            y_res
        );
        request_touch_pipeline_reset();
        publish_touch_status(TouchStatus::Ready { x_res, y_res }).await;
    }

    loop {
        // Sample at the loop boundary so the idle recovery path contributes
        // stack evidence too. Sampling only after a GPIO assertion leaves the
        // metric unset on an untouched device because RecoveryDue continues
        // before reaching the interaction-completion path below.
        if Instant::now() >= stack_sample_at {
            crate::firmware::observability::record_touch_core_stack_headroom();
            stack_sample_at = Instant::now() + Duration::from_secs(1);
        }

        let now = Instant::now();
        if matches!(mode, Gpio36Mode::SharedWithTouch) && !touch_ready && now >= retry_at {
            publish_touch_status(TouchStatus::Initializing).await;
            match touch.init_with_status().await {
                Ok(TouchInitStatus::Ready { x_res, y_res }) => {
                    console::println!(
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
                    console::println!(
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
                        "poll",
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
        console::println!("input: gpio36 raw=low");
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
        console::println!("input: gpio36 raw=high");
        publish_action(classifier.on_released()).await;
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
        TouchAcquisitionCommand::ReplayPipelineTap => {
            run_pipeline_replay_probe(classifier, contact_sampling).await;
            return;
        }
        TouchAcquisitionCommand::Suspend => {}
    }

    classifier.cancel_pending();
    let touch_powered_down = if POWER_DOWN_DURING_PANEL_DIAGNOSTIC && *touch_ready {
        match touch.shutdown().await {
            Ok(()) => {
                // Let the controller supply and interrupt output settle before
                // acknowledging the panel's quiet window.
                Timer::after_millis(50).await;
                console::println!("touch: panel_quiet power=off status=ok");
                true
            }
            Err(error) => {
                console::println!(
                    "touch: panel_quiet power=off status=error error={:?}",
                    error
                );
                false
            }
        }
    } else {
        false
    };
    TOUCH_ACQUISITION_STATE.signal(TouchAcquisitionState::Suspended);
    hard_park_touch_core_until_resume();
    let reset_pipeline = loop {
        let command = TOUCH_ACQUISITION_COMMANDS.receive().await;
        match command {
            TouchAcquisitionCommand::Suspend => {
                TOUCH_ACQUISITION_STATE.signal(TouchAcquisitionState::Suspended);
            }
            TouchAcquisitionCommand::Resume { reset_pipeline } => {
                break reset_pipeline;
            }
            TouchAcquisitionCommand::ReplayPipelineTap => {
                console::println!(
                    "TOUCH_PIPELINE_REPLAY state=rejected reason=acquisition_suspended"
                );
            }
        }
    };

    if touch_powered_down {
        publish_touch_status(TouchStatus::Initializing).await;
        match touch.init_with_status().await {
            Ok(TouchInitStatus::Ready { x_res, y_res }) => {
                *touch_ready = true;
                request_touch_pipeline_reset();
                publish_touch_status(TouchStatus::Ready { x_res, y_res }).await;
                console::println!(
                    "touch: panel_quiet power=on status=ready x_res={} y_res={}",
                    x_res,
                    y_res
                );
            }
            status => {
                console::println!("touch: panel_quiet power=on status=error init={:?}", status);
                handle_fault(touch, touch_ready, retry_at).await;
            }
        }
    } else if *touch_ready {
        if reset_pipeline {
            if touch.read_sample(0).await.is_err() {
                handle_fault(touch, touch_ready, retry_at).await;
            }
            request_touch_pipeline_reset();
        } else if read_and_publish(
            touch,
            Instant::now().as_millis(),
            "resume",
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

async fn run_pipeline_replay_probe(
    classifier: &mut Gpio36Classifier,
    contact_sampling: &mut ContactSamplingState,
) {
    classifier.cancel_pending();
    *contact_sampling = ContactSamplingState::new();
    reset_touch_pipeline().await;

    let started_ms = Instant::now().as_millis();
    console::println!(
        "TOUCH_PIPELINE_REPLAY state=started core=1 frames={} x={} y={}",
        PIPELINE_REPLAY_TAP.len(),
        PIPELINE_REPLAY_TAP[0].x,
        PIPELINE_REPLAY_TAP[0].y,
    );

    for (index, replay) in PIPELINE_REPLAY_TAP.iter().copied().enumerate() {
        let due_ms = started_ms.saturating_add(replay.offset_ms);
        let now_ms = Instant::now().as_millis();
        if due_ms > now_ms {
            Timer::after_millis(due_ms - now_ms).await;
        }

        let t_ms = Instant::now().as_millis();
        let sample = TouchSample {
            touch_count: replay.touch_count,
            points: [
                TouchPoint {
                    x: replay.x,
                    y: replay.y,
                },
                TouchPoint::default(),
            ],
            raw: replay.raw,
        };
        contact_sampling.record_authoritative_count(replay.touch_count);
        scheduling::record_sample(t_ms, replay.touch_count);
        push_touch_input_sample(TouchSampleFrame { t_ms, sample }).await;
        console::println!(
            "TOUCH_PIPELINE_REPLAY state=frame index={} offset_ms={} t_ms={} count={} raw_mask={:#04x}",
            index,
            replay.offset_ms,
            t_ms,
            replay.touch_count,
            replay.raw[7] & 0x03,
        );
    }

    console::println!(
        "TOUCH_PIPELINE_REPLAY state=frames_complete elapsed_ms={}",
        Instant::now().as_millis().saturating_sub(started_ms)
    );
}

fn hard_park_touch_core_until_resume() {
    if !HARD_PARK_DURING_PANEL_DIAGNOSTIC {
        return;
    }

    TOUCH_CORE_HARD_PARK_RESUME.store(false, Ordering::Release);
    let interrupt_lock = SingleCoreInterruptLock;
    let interrupt_state = unsafe { interrupt_lock.enter() };
    TOUCH_CORE_HARD_PARKED.store(true, Ordering::Release);
    while !TOUCH_CORE_HARD_PARK_RESUME.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }
    TOUCH_CORE_HARD_PARKED.store(false, Ordering::Release);
    unsafe { interrupt_lock.exit(interrupt_state) };
}
