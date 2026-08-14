use core::sync::atomic::Ordering;

use embassy_futures::select::{select3, Either3};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Duration, Instant, Ticker};

use super::super::{
    config::{
        TOUCH_CONTROLLER_ACTIVE_SLOTS, TOUCH_EVENT_TRACE_ENABLED, TOUCH_EVENT_TRACE_SAMPLES,
        TOUCH_IMU_ACTIVITY, TOUCH_LVGL_MULTITOUCH_FRAMES, TOUCH_LVGL_MULTITOUCH_RESET,
        TOUCH_PIPELINE_EVENTS, TOUCH_PIPELINE_INPUTS, TOUCH_SAMPLE_ACTIVE_MS,
    },
    imu_activity::snapshot_for_event,
    lvgl_multitouch::{LvglMultitouchFrame, LvglTouchPoint},
    types::{TouchActivitySnapshot, TouchEvent, TouchPipelineInput, TouchSampleFrame},
    TouchEngine,
};
#[cfg(not(feature = "wifi-debug-slim-app"))]
use super::super::{
    config::{TOUCH_TRACE_ENABLED, TOUCH_TRACE_SAMPLES},
    types::TouchTraceSample,
};

#[derive(Clone, Copy)]
enum TouchPipelineCommand {
    Suspend,
    Resume,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TouchPipelineState {
    Suspended,
    Running,
}

static TOUCH_PIPELINE_COMMANDS: Channel<CriticalSectionRawMutex, TouchPipelineCommand, 2> =
    Channel::new();
static TOUCH_PIPELINE_STATE: Signal<CriticalSectionRawMutex, TouchPipelineState> = Signal::new();

pub(crate) async fn suspend_touch_pipeline() {
    TOUCH_PIPELINE_COMMANDS
        .send(TouchPipelineCommand::Suspend)
        .await;
    while TOUCH_PIPELINE_STATE.wait().await != TouchPipelineState::Suspended {}
}

pub(crate) async fn resume_touch_pipeline() {
    request_touch_pipeline_resume().await;
    while TOUCH_PIPELINE_STATE.wait().await != TouchPipelineState::Running {}
}

pub(crate) async fn request_touch_pipeline_resume() {
    TOUCH_PIPELINE_COMMANDS
        .send(TouchPipelineCommand::Resume)
        .await;
}

pub(crate) fn try_request_touch_pipeline_resume() -> bool {
    TOUCH_PIPELINE_COMMANDS
        .try_send(TouchPipelineCommand::Resume)
        .is_ok()
}

#[embassy_executor::task]
pub(crate) async fn touch_pipeline_task() {
    let mut engine = TouchEngine::default();
    let mut ticker = Ticker::every(Duration::from_millis(TOUCH_SAMPLE_ACTIVE_MS));
    let mut multitouch_active = false;
    let mut multitouch_delivery_failed = false;
    let mut suspended = false;
    TOUCH_PIPELINE_STATE.signal(TouchPipelineState::Running);

    loop {
        if suspended {
            apply_pipeline_command(TOUCH_PIPELINE_COMMANDS.receive().await, &mut suspended);
            continue;
        }
        if let Ok(command) = TOUCH_PIPELINE_COMMANDS.try_receive() {
            apply_pipeline_command(command, &mut suspended);
            continue;
        }
        match select3(
            TOUCH_PIPELINE_COMMANDS.receive(),
            TOUCH_PIPELINE_INPUTS.receive(),
            ticker.next(),
        )
        .await
        {
            Either3::First(command) => apply_pipeline_command(command, &mut suspended),
            Either3::Second(input) => {
                process_input(
                    &mut engine,
                    input,
                    &mut multitouch_active,
                    &mut multitouch_delivery_failed,
                )
                .await
            }
            Either3::Third(_) => {
                emit_output(&mut engine, Instant::now().as_millis(), true).await;
            }
        }
    }
}

fn apply_pipeline_command(command: TouchPipelineCommand, suspended: &mut bool) {
    match command {
        TouchPipelineCommand::Suspend => {
            *suspended = true;
            TOUCH_PIPELINE_STATE.signal(TouchPipelineState::Suspended);
        }
        TouchPipelineCommand::Resume => {
            *suspended = false;
            TOUCH_PIPELINE_STATE.signal(TouchPipelineState::Running);
        }
    }
}

async fn process_input(
    engine: &mut TouchEngine,
    input: TouchPipelineInput,
    multitouch_active: &mut bool,
    multitouch_delivery_failed: &mut bool,
) {
    match input {
        TouchPipelineInput::Reset => {
            *engine = TouchEngine::default();
            while TOUCH_PIPELINE_EVENTS.try_receive().is_ok() {}
            while TOUCH_LVGL_MULTITOUCH_FRAMES.try_receive().is_ok() {}
            TOUCH_LVGL_MULTITOUCH_RESET.store(true, Ordering::Release);
            TOUCH_CONTROLLER_ACTIVE_SLOTS.store(0, Ordering::Release);
            *multitouch_active = false;
            *multitouch_delivery_failed = true;
            TOUCH_IMU_ACTIVITY.signal(TouchActivitySnapshot::default());
        }
        TouchPipelineInput::Sample(frame) => {
            forward_multitouch_frame(frame, multitouch_active, multitouch_delivery_failed);
            #[cfg(not(feature = "wifi-debug-slim-app"))]
            if TOUCH_TRACE_ENABLED && frame.sample.touch_count > 0 {
                let _ = TOUCH_TRACE_SAMPLES
                    .try_send(TouchTraceSample::from_sample(frame.t_ms, frame.sample));
            }
            let output = engine.tick(frame.t_ms, frame.sample);
            push_events(output.events).await;
        }
    }
}

fn forward_multitouch_frame(
    frame: TouchSampleFrame,
    multitouch_active: &mut bool,
    delivery_failed: &mut bool,
) {
    let active_mask = crate::platform::inkplate::touch::active_slots(&frame.sample.raw);
    if crate::platform::inkplate::touch::is_touch_report(&frame.sample.raw)
        || frame.sample.touch_count == 0
    {
        TOUCH_CONTROLLER_ACTIVE_SLOTS.store(active_mask, Ordering::Release);
    }
    let lvgl_frame = LvglMultitouchFrame {
        t_ms: frame.t_ms,
        active_mask,
        points: frame.sample.points.map(|point| LvglTouchPoint {
            x: point.x,
            y: point.y,
        }),
    };
    let current_multitouch = lvgl_frame.is_multitouch();

    if *delivery_failed {
        if !current_multitouch && !TOUCH_LVGL_MULTITOUCH_RESET.load(Ordering::Acquire) {
            *delivery_failed = false;
        }
        *multitouch_active = current_multitouch;
        return;
    }

    if (current_multitouch || *multitouch_active)
        && TOUCH_LVGL_MULTITOUCH_FRAMES.try_send(lvgl_frame).is_err()
    {
        TOUCH_LVGL_MULTITOUCH_RESET.store(true, Ordering::Release);
        *delivery_failed = true;
    }
    *multitouch_active = current_multitouch;
}

async fn emit_output(engine: &mut TouchEngine, t_ms: u64, advance: bool) {
    if advance {
        let output = engine.advance(t_ms);
        push_events(output.events).await;
    }
}

async fn push_events(events: [Option<TouchEvent>; 3]) {
    for event in events.into_iter().flatten() {
        publish_imu_activity(event);
        if TOUCH_EVENT_TRACE_ENABLED {
            let _ = TOUCH_EVENT_TRACE_SAMPLES.try_send(event);
        }
        TOUCH_PIPELINE_EVENTS.send(event).await;
    }
}

fn publish_imu_activity(event: TouchEvent) {
    TOUCH_IMU_ACTIVITY.signal(snapshot_for_event(event));
}

pub(crate) async fn push_touch_input_sample(frame: TouchSampleFrame) {
    TOUCH_PIPELINE_INPUTS
        .send(TouchPipelineInput::Sample(frame))
        .await;
}

pub(crate) fn request_touch_pipeline_reset() {
    let _ = TOUCH_PIPELINE_INPUTS.try_send(TouchPipelineInput::Reset);
}

pub(crate) async fn reset_touch_pipeline() {
    TOUCH_PIPELINE_INPUTS.send(TouchPipelineInput::Reset).await;
}
