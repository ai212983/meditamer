use embassy_futures::select::{select, Either};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Duration, Instant, Timer};

mod runtime;
mod state;

use crate::firmware::{
    input::gpio36::{Gpio36Classifier, Gpio36Mode},
    types::{Gpio36InputPin, InkplateTouchDriver, TouchSampleFrame, TouchStatus},
};
use inkplate_tempera::{TouchInitStatus, TouchPoint, TouchSample};

use super::{push_touch_input_sample, request_touch_pipeline_reset};
use crate::firmware::bounded_control::{
    Control, ControlAck, ControlCommand, ControlRequest, SuspendAck,
};
use crate::firmware::touch::{
    config::{GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED, TOUCH_INIT_RETRY_MS},
    replay::PIPELINE_REPLAY_TAP,
    scheduling,
};
use runtime::{
    flush_pending_status, handle_fault, probe_asserted_line, publish_classifier_action,
    publish_touch_status, read_and_publish, wait_for_assertion_or_recovery, AcquisitionWake,
    ProbeResult,
};
use state::ContactSamplingState;

const POWER_DOWN_DURING_PANEL_DIAGNOSTIC: bool =
    option_env!("MEDITAMER_TOUCH_POWER_DOWN_DURING_PANEL").is_some();
const _: () = assert!(
    option_env!("MEDITAMER_TOUCH_CORE_HARD_PARK").is_none(),
    "touch-only hard parking is incompatible with the shared CPU1 I2C owner"
);
static DEFERRED_CONTROL: Signal<CriticalSectionRawMutex, TouchAcquisitionCommand> = Signal::new();
static TOUCH_ACQUISITION_CONTROL: Control<CriticalSectionRawMutex> = Control::new();
// Replay is an event, not desired state; keep each admitted probe independently.
static TOUCH_REPLAY_REQUESTS: Channel<CriticalSectionRawMutex, (), 2> = Channel::new();

#[derive(Clone, Copy)]
enum TouchAcquisitionCommand {
    Control(ControlRequest),
    ReplayPipelineTap,
}

/// Timing out stops the wait; the latest desired state remains pending.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);

/// Confirms acquisition quiescence without blocking the CPU1 bus owner.
pub async fn suspend_touch_acquisition() -> SuspendAck {
    TOUCH_ACQUISITION_CONTROL
        .suspend(Timer::after(CONTROL_TIMEOUT))
        .await
}

/// Publishes persistent Running intent, then waits for the post-resume
/// controller work and reset request to complete.
pub async fn resume_touch_acquisition(reset_pipeline: bool) -> bool {
    TOUCH_ACQUISITION_CONTROL
        .resume(reset_pipeline, Timer::after(CONTROL_TIMEOUT))
        .await
}

/// Opens waveform sampling without waiting for a post-resume controller read.
/// Admission is synchronous; CPU1 remains available to the shared bus owner.
pub async fn request_touch_acquisition_resume(reset_pipeline: bool) -> bool {
    try_request_touch_acquisition_resume(reset_pipeline)
}

pub fn try_request_touch_acquisition_resume(reset_pipeline: bool) -> bool {
    TOUCH_ACQUISITION_CONTROL.request_resume(reset_pipeline);
    true
}

pub async fn request_touch_pipeline_replay_probe() {
    TOUCH_REPLAY_REQUESTS.send(()).await;
}

fn try_receive_command() -> Option<TouchAcquisitionCommand> {
    if let Some(command) = DEFERRED_CONTROL.try_take() {
        return Some(command);
    }
    TOUCH_ACQUISITION_CONTROL
        .try_receive()
        .map(TouchAcquisitionCommand::Control)
        .or_else(|| {
            TOUCH_REPLAY_REQUESTS
                .try_receive()
                .ok()
                .map(|()| TouchAcquisitionCommand::ReplayPipelineTap)
        })
}

async fn receive_command() -> TouchAcquisitionCommand {
    if let Some(command) = DEFERRED_CONTROL.try_take() {
        return command;
    }
    match select(
        TOUCH_ACQUISITION_CONTROL.receive(),
        TOUCH_REPLAY_REQUESTS.receive(),
    )
    .await
    {
        Either::First(request) => TouchAcquisitionCommand::Control(request),
        Either::Second(()) => TouchAcquisitionCommand::ReplayPipelineTap,
    }
}

#[embassy_executor::task]
pub async fn touch_acquisition_task(
    mut touch: InkplateTouchDriver,
    mut retry_timer: crate::firmware::types::TouchRetryTimer,
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

        flush_pending_status().await;

        // A pending quiet-window request wins over controller initialization.
        if let Some(command) = try_receive_command() {
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
            match select(receive_command(), Timer::after_millis(wait_ms)).await {
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
        match wait_for_assertion_or_recovery(
            &mut gpio36,
            &mut retry_timer,
            contact_sampling.poll_delay_ms(Instant::now().as_millis()),
        )
        .await
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
                        &mut retry_timer,
                        Instant::now().as_millis(),
                        "poll",
                        &mut classifier,
                        &mut contact_sampling,
                    )
                    .await
                    .is_err()
                {
                    handle_fault(&mut touch, &mut touch_ready, &mut retry_at, &mut classifier)
                        .await;
                }
                continue;
            }
            AcquisitionWake::Asserted => {}
        }

        let edge_ms = Instant::now().as_millis();
        #[cfg(feature = "firmware-trace")]
        console::println!("input: gpio36 raw=low");
        let action = classifier.on_asserted(edge_ms, mode);
        publish_classifier_action(&classifier, action).await;

        if touch_ready {
            match probe_asserted_line(
                &mut touch,
                &mut retry_timer,
                &gpio36,
                &mut classifier,
                &mut contact_sampling,
            )
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
                    handle_fault(&mut touch, &mut touch_ready, &mut retry_at, &mut classifier)
                        .await;
                    continue;
                }
            }
        }

        match select(receive_command(), gpio36.wait_for_high()).await {
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
        #[cfg(feature = "firmware-trace")]
        console::println!("input: gpio36 raw=high");
        let release_ms = Instant::now().as_millis();
        let action = classifier.on_released(release_ms);
        publish_classifier_action(&classifier, action).await;
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
    let mut request = match command {
        TouchAcquisitionCommand::Control(request) => request,
        TouchAcquisitionCommand::ReplayPipelineTap => {
            run_pipeline_replay_probe(classifier, contact_sampling).await;
            return;
        }
    };
    let mut suspended = false;
    let mut touch_powered_down = false;
    loop {
        if TOUCH_ACQUISITION_CONTROL.is_current(request) {
            match request.command {
                ControlCommand::Suspend => {
                    if !suspended {
                        // Neither a pending classification nor an
                        // already-accepted WAKE press may survive suspend --
                        // see `handle_fault`'s use of the same call. The
                        // snapshot mirrors the cancellation so the display
                        // can never toggle the ended hold later.
                        classifier.cancel_accepted();
                        crate::firmware::input::gpio36::sync_wake_snapshot(classifier);
                        touch_powered_down = power_down_for_panel(touch, *touch_ready).await;
                        suspended = true;
                    }
                    crate::firmware::acquisition_metrics::TOUCH.pause();
                    scheduling::pause();
                    TOUCH_ACQUISITION_CONTROL.acknowledge(request, ControlAck::Quiesced);
                }
                ControlCommand::Resume { reset_pipeline } => {
                    apply_resume(
                        touch,
                        touch_ready,
                        retry_at,
                        classifier,
                        touch_powered_down,
                        reset_pipeline,
                    )
                    .await;
                    TOUCH_ACQUISITION_CONTROL.acknowledge(request, ControlAck::Running);
                    return;
                }
            }
        }
        request = loop {
            match receive_command().await {
                TouchAcquisitionCommand::Control(next) => break next,
                TouchAcquisitionCommand::ReplayPipelineTap => {
                    console::println!(
                        "TOUCH_PIPELINE_REPLAY state=rejected reason=acquisition_suspended"
                    );
                }
            }
        };
    }
}

async fn power_down_for_panel(touch: &mut InkplateTouchDriver, touch_ready: bool) -> bool {
    if !(POWER_DOWN_DURING_PANEL_DIAGNOSTIC && touch_ready) {
        return false;
    }
    match touch.shutdown().await {
        Ok(()) => {
            // Let the controller supply and interrupt output settle before ack.
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
}

async fn apply_resume(
    touch: &mut InkplateTouchDriver,
    touch_ready: &mut bool,
    retry_at: &mut Instant,
    classifier: &mut Gpio36Classifier,
    touch_powered_down: bool,
    reset_pipeline: bool,
) {
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
                handle_fault(touch, touch_ready, retry_at, classifier).await;
                if reset_pipeline {
                    request_touch_pipeline_reset();
                }
            }
        }
    } else if *touch_ready {
        if reset_pipeline {
            if touch.read_sample(0).await.is_err() {
                handle_fault(touch, touch_ready, retry_at, classifier).await;
            }
            request_touch_pipeline_reset();
        }
        // The next acquisition turn reads/publishes after Running is acknowledged.
        // A full CPU0 sample queue cannot block this control acknowledgement.
    } else if reset_pipeline {
        request_touch_pipeline_reset();
    }
}

async fn run_pipeline_replay_probe(
    classifier: &mut Gpio36Classifier,
    contact_sampling: &mut ContactSamplingState,
) {
    classifier.cancel_pending();
    *contact_sampling = ContactSamplingState::new();
    request_touch_pipeline_reset();

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
            if !runtime::publish_with_control(Timer::after_millis(due_ms - now_ms)).await {
                request_touch_pipeline_reset();
                return;
            }
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
        if !runtime::publish_with_control(push_touch_input_sample(TouchSampleFrame {
            admission_epoch: crate::firmware::touch::admission::epoch(),
            #[cfg(feature = "ui-interaction-trace")]
            trace_id: crate::firmware::interaction_trace::sample(
                t_ms,
                sample.touch_count,
                sample.points[0].x,
                sample.points[0].y,
            ),
            #[cfg(feature = "ui-interaction-trace")]
            trace_frame_id: crate::firmware::touch::interaction_trace::next_frame_id(),
            t_ms,
            sample,
        }))
        .await
        {
            request_touch_pipeline_reset();
            return;
        }
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
