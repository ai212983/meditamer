use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Instant, Ticker};

use super::super::{
    config::{
        TOUCH_EVENT_TRACE_ENABLED, TOUCH_EVENT_TRACE_SAMPLES, TOUCH_IMU_ACTIVITY,
        TOUCH_PIPELINE_EVENTS, TOUCH_PIPELINE_INPUTS, TOUCH_SAMPLE_ACTIVE_MS,
    },
    imu_activity::snapshot_for_event,
    types::{TouchActivitySnapshot, TouchEvent, TouchPipelineInput, TouchSampleFrame},
    TouchEngine,
};
#[cfg(not(feature = "wifi-debug-slim-app"))]
use super::super::{
    config::{TOUCH_TRACE_ENABLED, TOUCH_TRACE_SAMPLES},
    types::TouchTraceSample,
};

#[embassy_executor::task]
pub(crate) async fn touch_pipeline_task() {
    let mut engine = TouchEngine::default();
    let mut ticker = Ticker::every(Duration::from_millis(TOUCH_SAMPLE_ACTIVE_MS));

    loop {
        match select(TOUCH_PIPELINE_INPUTS.receive(), ticker.next()).await {
            Either::First(input) => process_input(&mut engine, input).await,
            Either::Second(_) => {
                emit_output(&mut engine, Instant::now().as_millis(), true).await;
            }
        }
    }
}

async fn process_input(engine: &mut TouchEngine, input: TouchPipelineInput) {
    match input {
        TouchPipelineInput::Reset => {
            *engine = TouchEngine::default();
            while TOUCH_PIPELINE_EVENTS.try_receive().is_ok() {}
            TOUCH_IMU_ACTIVITY.signal(TouchActivitySnapshot::default());
        }
        TouchPipelineInput::Sample(frame) => {
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
