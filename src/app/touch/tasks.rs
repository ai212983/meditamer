use core::sync::atomic::Ordering;

use meditamer::inkplate_hal::TouchInitStatus;

use super::{
    config::{
        TOUCH_EVENT_TRACE_ENABLED, TOUCH_EVENT_TRACE_SAMPLES, TOUCH_FEEDBACK_RADIUS_PX,
        TOUCH_IRQ_LOW, TOUCH_PIPELINE_EVENTS, TOUCH_PIPELINE_INPUTS, TOUCH_SAMPLE_ACTIVE_MS,
        TOUCH_SAMPLE_IDLE_MS, TOUCH_TRACE_ENABLED, TOUCH_TRACE_SAMPLES,
    },
    types::{TouchEvent, TouchIrqPin, TouchPipelineInput, TouchSampleFrame, TouchTraceSample},
    TouchEngine,
};
use crate::app::{
    config::APP_EVENTS,
    types::{AppEvent, InkplateDriver},
};

#[embassy_executor::task]
pub(crate) async fn touch_irq_task(mut touch_irq: TouchIrqPin) {
    loop {
        touch_irq.wait_for_falling_edge().await;
        TOUCH_IRQ_LOW.store(true, Ordering::Relaxed);
        let _ = APP_EVENTS.try_send(AppEvent::TouchIrq);
        // Re-arm on level return so held-low periods don't starve next edge.
        if touch_irq.is_low() {
            touch_irq.wait_for_rising_edge().await;
        }
        TOUCH_IRQ_LOW.store(false, Ordering::Relaxed);
    }
}

#[embassy_executor::task]
pub(crate) async fn touch_pipeline_task() {
    let mut touch_engine = TouchEngine::default();

    loop {
        match TOUCH_PIPELINE_INPUTS.receive().await {
            TouchPipelineInput::Reset => {
                touch_engine = TouchEngine::default();
                while TOUCH_PIPELINE_EVENTS.try_receive().is_ok() {}
            }
            TouchPipelineInput::Sample(frame) => {
                if TOUCH_TRACE_ENABLED && frame.sample.touch_count > 0 {
                    let _ = TOUCH_TRACE_SAMPLES
                        .try_send(TouchTraceSample::from_sample(frame.t_ms, frame.sample));
                }

                let output = touch_engine.tick(frame.t_ms, frame.sample);
                for touch_event in output.events.into_iter().flatten() {
                    if TOUCH_EVENT_TRACE_ENABLED {
                        let _ = TOUCH_EVENT_TRACE_SAMPLES.try_send(touch_event);
                    }
                    push_touch_output_event(touch_event).await;
                }
            }
        }
    }
}

pub(crate) async fn push_touch_input_sample(frame: TouchSampleFrame) {
    let input = TouchPipelineInput::Sample(frame);
    // Preserve ordered sample stream; dropping old samples collapses swipe vectors.
    TOUCH_PIPELINE_INPUTS.send(input).await;
}

pub(crate) fn request_touch_pipeline_reset() {
    while TOUCH_PIPELINE_INPUTS.try_receive().is_ok() {}
    while TOUCH_PIPELINE_EVENTS.try_receive().is_ok() {}
    let _ = TOUCH_PIPELINE_INPUTS.try_send(TouchPipelineInput::Reset);
}

pub(crate) fn next_touch_sample_period_ms(touch_active: bool) -> u64 {
    if touch_active {
        TOUCH_SAMPLE_ACTIVE_MS
    } else {
        TOUCH_SAMPLE_IDLE_MS
    }
}

async fn push_touch_output_event(event: TouchEvent) {
    TOUCH_PIPELINE_EVENTS.send(event).await;
}

pub(crate) fn try_touch_init_with_logs(display: &mut InkplateDriver, phase: &str) -> bool {
    match display.touch_init_with_status() {
        Ok(TouchInitStatus::Ready { x_res, y_res }) => {
            esp_println::println!(
                "touch: ready phase={} x_res={} y_res={}",
                phase,
                x_res,
                y_res
            );
            true
        }
        Ok(TouchInitStatus::HelloMismatch { hello }) => {
            let probes = display.probe_devices();
            esp_println::println!(
                "touch: init_failed phase={} reason=hello_mismatch hello={:02x}{:02x}{:02x}{:02x} probe_int={} probe_ext={} probe_pwr={}",
                phase,
                hello[0],
                hello[1],
                hello[2],
                hello[3],
                probes.io_internal,
                probes.io_external,
                probes.tps65186
            );
            false
        }
        Ok(TouchInitStatus::ZeroResolution { x_res, y_res }) => {
            let probes = display.probe_devices();
            esp_println::println!(
                "touch: init_failed phase={} reason=zero_resolution x_res={} y_res={} probe_int={} probe_ext={} probe_pwr={}",
                phase,
                x_res,
                y_res,
                probes.io_internal,
                probes.io_external,
                probes.tps65186
            );
            false
        }
        Err(_) => {
            let probes = display.probe_devices();
            esp_println::println!(
                "touch: init_failed phase={} reason=i2c_error probe_int={} probe_ext={} probe_pwr={}",
                phase,
                probes.io_internal,
                probes.io_external,
                probes.tps65186
            );
            false
        }
    }
}

pub(crate) fn draw_touch_feedback_dot(display: &mut InkplateDriver, x: u16, y: u16) {
    let cx = x as i32;
    let cy = y as i32;
    let radius = TOUCH_FEEDBACK_RADIUS_PX.max(1);
    let radius_sq = radius * radius;
    let width = display.width() as i32;
    let height = display.height() as i32;

    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy > radius_sq {
                continue;
            }

            let px = cx + dx;
            let py = cy + dy;
            if px < 0 || py < 0 || px >= width || py >= height {
                continue;
            }

            display.set_pixel_bw(px as usize, py as usize, true);
        }
    }
}
