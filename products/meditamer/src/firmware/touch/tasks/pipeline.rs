use core::sync::atomic::Ordering;

use core::future::pending;
#[cfg(feature = "cpu-load")]
use cpu_load::profile::{PipelineBranch, PipelinePhase};
use embassy_futures::select::{select3, Either3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Instant, Timer};

use super::super::{
    config::{
        TOUCH_CONTROLLER_ACTIVE_SLOTS, TOUCH_EVENT_TRACE_ENABLED, TOUCH_EVENT_TRACE_SAMPLES,
        TOUCH_IMU_ACTIVITY, TOUCH_LVGL_MULTITOUCH_FRAMES, TOUCH_LVGL_MULTITOUCH_RESET,
        TOUCH_PIPELINE_EVENTS, TOUCH_PIPELINE_INPUTS, TOUCH_PIPELINE_RESET_PENDING,
        TOUCH_SAMPLE_ACTIVE_MS,
    },
    core::ContactSuppressionGate,
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
use crate::firmware::bounded_control::{
    Control, ControlAck, ControlCommand, ControlRequest, SuspendAck,
};

static TOUCH_PIPELINE_CONTROL: Control<CriticalSectionRawMutex> = Control::new();

/// Timing out stops the wait; the latest desired state remains pending.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);

/// Confirms quiescence for this exact request within the caller's budget.
pub async fn suspend_touch_pipeline() -> SuspendAck {
    TOUCH_PIPELINE_CONTROL
        .suspend(Timer::after(CONTROL_TIMEOUT))
        .await
}

/// Publishes persistent Running intent before waiting for acknowledgement.
pub async fn resume_touch_pipeline() -> bool {
    TOUCH_PIPELINE_CONTROL
        .resume(false, Timer::after(CONTROL_TIMEOUT))
        .await
}

pub fn try_request_touch_pipeline_resume() -> bool {
    TOUCH_PIPELINE_CONTROL.request_resume(false);
    true
}

#[embassy_executor::task]
pub async fn touch_pipeline_task() {
    crate::firmware::acquisition_metrics::start_delivery_window(Instant::now().as_millis());
    let mut engine = TouchEngine::default();
    let mut next_tick = Instant::now() + Duration::from_millis(TOUCH_SAMPLE_ACTIVE_MS);
    let mut multitouch_active = false;
    let mut multitouch_delivery_failed = false;
    let mut contact_suppression = ContactSuppressionGate::new();
    let mut active_admission_epoch = crate::firmware::touch::admission::epoch();
    #[cfg(feature = "ui-interaction-trace")]
    let mut trace = crate::firmware::touch::interaction_trace::GestureCorrelation::default();
    let mut suspended = false;
    loop {
        if suspended {
            apply_pipeline_command(TOUCH_PIPELINE_CONTROL.receive().await, &mut suspended);
            continue;
        }
        if let Some(command) = TOUCH_PIPELINE_CONTROL.try_receive() {
            apply_pipeline_command(command, &mut suspended);
            continue;
        }
        let current_epoch = crate::firmware::touch::admission::epoch();
        if current_epoch != active_admission_epoch {
            retire_admission_state(
                &mut engine,
                &mut contact_suppression,
                &mut multitouch_active,
                &mut multitouch_delivery_failed,
                #[cfg(feature = "ui-interaction-trace")]
                &mut trace,
            );
            active_admission_epoch = current_epoch;
        }
        // A pending reset is serviced before any later sample.
        if TOUCH_PIPELINE_RESET_PENDING.load(Ordering::Acquire) != 0 {
            process_input(
                &mut engine,
                TouchPipelineInput::Reset,
                &mut multitouch_active,
                &mut multitouch_delivery_failed,
                &mut contact_suppression,
                &mut active_admission_epoch,
                #[cfg(feature = "ui-interaction-trace")]
                &mut trace,
            )
            .await;
            continue;
        }
        let active = engine.needs_tick();
        if !active {
            next_tick = Instant::now() + Duration::from_millis(TOUCH_SAMPLE_ACTIVE_MS);
        }
        match select3(
            TOUCH_PIPELINE_CONTROL.receive(),
            TOUCH_PIPELINE_INPUTS.receive(),
            async {
                if active {
                    Timer::at(next_tick).await;
                } else {
                    pending::<()>().await;
                }
            },
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
                    &mut contact_suppression,
                    &mut active_admission_epoch,
                    #[cfg(feature = "ui-interaction-trace")]
                    &mut trace,
                )
                .await
            }
            Either3::Third(_) => {
                #[cfg(feature = "cpu-load")]
                cpu_load::tail_note_pipeline(PipelineBranch::Tick);
                emit_output(
                    &mut engine,
                    Instant::now().as_millis(),
                    true,
                    active_admission_epoch,
                    #[cfg(feature = "ui-interaction-trace")]
                    &mut trace,
                )
                .await;
                next_tick += Duration::from_millis(TOUCH_SAMPLE_ACTIVE_MS);
                if next_tick <= Instant::now() {
                    next_tick = Instant::now() + Duration::from_millis(TOUCH_SAMPLE_ACTIVE_MS);
                }
            }
        }
    }
}

fn apply_pipeline_command(request: ControlRequest, suspended: &mut bool) {
    if !TOUCH_PIPELINE_CONTROL.is_current(request) {
        return;
    }
    match request.command {
        ControlCommand::Suspend => {
            *suspended = true;
            TOUCH_PIPELINE_CONTROL.acknowledge(request, ControlAck::Quiesced);
        }
        ControlCommand::Resume { .. } => {
            *suspended = false;
            crate::firmware::acquisition_metrics::start_delivery_window(Instant::now().as_millis());
            TOUCH_PIPELINE_CONTROL.acknowledge(request, ControlAck::Running);
        }
    }
}

// Admission boundaries retire the HSM without generating a cancellation for
// the destination. LVGL's local state was already retired by navigation.
fn retire_admission_state(
    engine: &mut TouchEngine,
    contact_suppression: &mut ContactSuppressionGate,
    multitouch_active: &mut bool,
    multitouch_delivery_failed: &mut bool,
    #[cfg(feature = "ui-interaction-trace")]
    trace: &mut crate::firmware::touch::interaction_trace::GestureCorrelation,
) {
    *engine = TouchEngine::default();
    *contact_suppression = ContactSuppressionGate::new();
    #[cfg(feature = "ui-interaction-trace")]
    {
        let _ = trace.reset_boundary();
        trace.finish_batch(true);
        trace.reconcile_engine(true);
    }
    TOUCH_CONTROLLER_ACTIVE_SLOTS.store(0, Ordering::Release);
    *multitouch_active = false;
    // Navigation already retired the LVGL tracker, and admission accounts
    // for held contacts. A fresh two-finger gesture is not a delivery fault.
    *multitouch_delivery_failed = false;
    TOUCH_IMU_ACTIVITY.signal(TouchActivitySnapshot::default());
}

async fn process_input(
    engine: &mut TouchEngine,
    input: TouchPipelineInput,
    multitouch_active: &mut bool,
    multitouch_delivery_failed: &mut bool,
    contact_suppression: &mut ContactSuppressionGate,
    active_admission_epoch: &mut u32,
    #[cfg(feature = "ui-interaction-trace")]
    trace: &mut crate::firmware::touch::interaction_trace::GestureCorrelation,
) {
    match input {
        TouchPipelineInput::Reset => {
            // Queue entries only wake the consumer. The count survives a full
            // queue, drained wakeups, and requests arriving during this reset.
            let reset_count = TOUCH_PIPELINE_RESET_PENDING.swap(0, Ordering::AcqRel);
            if reset_count == 0 {
                return;
            }
            #[cfg(feature = "cpu-load")]
            cpu_load::tail_note_pipeline(PipelineBranch::Reset);
            #[cfg(feature = "ui-interaction-trace")]
            let boundary = trace.reset_boundary();
            #[cfg(feature = "ui-interaction-trace")]
            crate::firmware::interaction_trace::record(
                boundary.id,
                4,
                Instant::now().as_millis(),
                boundary.generation,
                0,
            );
            while let Ok(_input) = TOUCH_PIPELINE_INPUTS.try_receive() {
                #[cfg(feature = "ui-interaction-trace")]
                if let TouchPipelineInput::Sample(frame) = _input {
                    crate::firmware::interaction_trace::record(
                        frame.trace_id,
                        11,
                        frame.t_ms,
                        frame.trace_frame_id,
                        boundary.generation,
                    );
                }
            }
            // Drain stale output before publishing the cancellation boundary.
            let cancelled = engine.cancel(Instant::now().as_millis());
            *engine = TouchEngine::default();
            #[cfg(feature = "ui-interaction-trace")]
            while let Ok(event) = TOUCH_PIPELINE_EVENTS.try_receive() {
                crate::firmware::interaction_trace::record(
                    event.trace_id,
                    12,
                    event.time_ms(),
                    crate::firmware::interaction_trace::kind(event.kind),
                    boundary.generation,
                );
            }
            #[cfg(not(feature = "ui-interaction-trace"))]
            while TOUCH_PIPELINE_EVENTS.try_receive().is_ok() {}
            #[cfg(feature = "ui-interaction-trace")]
            while let Ok(frame) = TOUCH_LVGL_MULTITOUCH_FRAMES.try_receive() {
                crate::firmware::interaction_trace::record(
                    frame.trace_id,
                    13,
                    frame.t_ms,
                    frame.trace_frame_id,
                    boundary.generation,
                );
            }
            #[cfg(not(feature = "ui-interaction-trace"))]
            while TOUCH_LVGL_MULTITOUCH_FRAMES.try_receive().is_ok() {}
            push_events(
                cancelled.events,
                *active_admission_epoch,
                #[cfg(feature = "ui-interaction-trace")]
                trace,
                #[cfg(feature = "ui-interaction-trace")]
                None,
            )
            .await;
            #[cfg(feature = "ui-interaction-trace")]
            trace.reconcile_engine(engine.is_idle());
            TOUCH_LVGL_MULTITOUCH_RESET.store(true, Ordering::Release);
            crate::firmware::display::wake();
            TOUCH_CONTROLLER_ACTIVE_SLOTS.store(0, Ordering::Release);
            *multitouch_active = false;
            *multitouch_delivery_failed = true;
            // Do not let a held contact reappear as fresh input after reset.
            contact_suppression.arm();
            #[cfg(feature = "ui-interaction-trace")]
            crate::firmware::interaction_trace::record(
                boundary.id,
                15,
                Instant::now().as_millis(),
                boundary.generation,
                0,
            );
            TOUCH_IMU_ACTIVITY.signal(TouchActivitySnapshot::default());
            crate::firmware::touch::admission::resets_completed(reset_count);
        }
        TouchPipelineInput::Sample(frame) => {
            #[cfg(feature = "cpu-load")]
            cpu_load::tail_note_pipeline(PipelineBranch::Sample);
            #[cfg(feature = "cpu-load")]
            cpu_load::tail_note_pipeline_phase(PipelinePhase::Prep);
            if !crate::firmware::touch::admission::accepts(frame.admission_epoch) {
                #[cfg(feature = "ui-interaction-trace")]
                crate::firmware::interaction_trace::record(
                    frame.trace_id,
                    38,
                    frame.t_ms,
                    frame.admission_epoch,
                    2,
                );
                return;
            }
            // select() may have slept through the entire transition. Reset
            // before this first admitted sample, not on the following loop.
            if *active_admission_epoch != frame.admission_epoch {
                retire_admission_state(
                    engine,
                    contact_suppression,
                    multitouch_active,
                    multitouch_delivery_failed,
                    #[cfg(feature = "ui-interaction-trace")]
                    trace,
                );
                *active_admission_epoch = frame.admission_epoch;
            }
            #[cfg(feature = "ui-interaction-trace")]
            let frame_trace = crate::firmware::touch::interaction_trace::FrameCorrelation {
                contact_id: frame.trace_id,
                frame_id: frame.trace_frame_id,
            };
            #[cfg(feature = "ui-interaction-trace")]
            crate::firmware::interaction_trace::record(
                frame.trace_id,
                2,
                frame.t_ms,
                frame.trace_frame_id,
                trace.reset_generation(),
            );
            crate::firmware::acquisition_metrics::record_delivery(
                frame.t_ms,
                Instant::now().as_millis(),
            );
            #[cfg(feature = "ui-interaction-trace")]
            let was_suppressed = contact_suppression.is_armed();
            if !contact_suppression.admit(frame.sample.touch_count) {
                #[cfg(feature = "ui-interaction-trace")]
                {
                    crate::firmware::interaction_trace::record(
                        if frame.trace_id != 0 {
                            frame.trace_id
                        } else {
                            trace.active_or_last_id()
                        },
                        3,
                        frame.t_ms,
                        frame.trace_frame_id,
                        trace.reset_generation(),
                    );
                    if was_suppressed && frame.sample.touch_count == 0 {
                        crate::firmware::interaction_trace::record(
                            trace.active_or_last_id(),
                            14,
                            frame.t_ms,
                            frame.trace_frame_id,
                            trace.reset_generation(),
                        );
                    }
                }
                return;
            }
            #[cfg(feature = "ui-interaction-trace")]
            trace.observe_frame(frame_trace);
            forward_multitouch_frame(frame, multitouch_active, multitouch_delivery_failed);
            #[cfg(not(feature = "wifi-debug-slim-app"))]
            if TOUCH_TRACE_ENABLED && frame.sample.touch_count > 0 {
                let _ = TOUCH_TRACE_SAMPLES
                    .try_send(TouchTraceSample::from_sample(frame.t_ms, frame.sample));
            }
            #[cfg(feature = "cpu-load")]
            cpu_load::tail_note_pipeline_phase(PipelinePhase::Engine);
            let output = engine.tick(frame.t_ms, frame.sample);
            #[cfg(feature = "cpu-load")]
            cpu_load::tail_note_pipeline_phase(PipelinePhase::Events);
            push_events(
                output.events,
                frame.admission_epoch,
                #[cfg(feature = "ui-interaction-trace")]
                trace,
                #[cfg(feature = "ui-interaction-trace")]
                Some(frame_trace),
            )
            .await;
            #[cfg(feature = "ui-interaction-trace")]
            trace.reconcile_engine(engine.is_idle());
        }
    }
}

fn forward_multitouch_frame(
    frame: TouchSampleFrame,
    multitouch_active: &mut bool,
    delivery_failed: &mut bool,
) {
    let active_mask = inkplate_tempera::touch::active_slots(&frame.sample.raw);
    if inkplate_tempera::touch::is_touch_report(&frame.sample.raw) || frame.sample.touch_count == 0
    {
        TOUCH_CONTROLLER_ACTIVE_SLOTS.store(active_mask, Ordering::Release);
    }
    let lvgl_frame = LvglMultitouchFrame {
        admission_epoch: frame.admission_epoch,
        #[cfg(feature = "ui-interaction-trace")]
        trace_id: frame.trace_id,
        #[cfg(feature = "ui-interaction-trace")]
        trace_frame_id: frame.trace_frame_id,
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
        crate::firmware::display::wake();
        *delivery_failed = true;
    }
    *multitouch_active = current_multitouch;
}

async fn emit_output(
    engine: &mut TouchEngine,
    t_ms: u64,
    advance: bool,
    admission_epoch: u32,
    #[cfg(feature = "ui-interaction-trace")]
    trace: &mut crate::firmware::touch::interaction_trace::GestureCorrelation,
) {
    if advance {
        let output = engine.advance(t_ms);
        push_events(
            output.events,
            admission_epoch,
            #[cfg(feature = "ui-interaction-trace")]
            trace,
            #[cfg(feature = "ui-interaction-trace")]
            None,
        )
        .await;
        #[cfg(feature = "ui-interaction-trace")]
        trace.reconcile_engine(engine.is_idle());
    }
}

async fn push_events(
    events: [Option<TouchEvent>; 3],
    admission_epoch: u32,
    #[cfg(feature = "ui-interaction-trace")]
    trace: &mut crate::firmware::touch::interaction_trace::GestureCorrelation,
    #[cfg(feature = "ui-interaction-trace")] frame: Option<
        crate::firmware::touch::interaction_trace::FrameCorrelation,
    >,
) {
    #[cfg(feature = "ui-interaction-trace")]
    let mut ends_gesture = false;
    for event in events.into_iter().flatten() {
        let mut event = event;
        event.admission_epoch = admission_epoch;
        if !crate::firmware::touch::admission::accepts(admission_epoch) {
            #[cfg(feature = "ui-interaction-trace")]
            crate::firmware::interaction_trace::record(
                event.trace_id,
                38,
                event.time_ms(),
                admission_epoch,
                3,
            );
            continue;
        }
        #[cfg(feature = "ui-interaction-trace")]
        let event = {
            event.trace_id = trace.event_id(event.kind, frame);
            ends_gesture |= crate::firmware::touch::interaction_trace::ends_gesture(event.kind);
            crate::firmware::interaction_trace::record(
                event.trace_id,
                5,
                event.time_ms(),
                crate::firmware::interaction_trace::kind(event.kind),
                (u32::from(event.x) << 16) | u32::from(event.y),
            );
            event
        };
        publish_imu_activity(event);
        if TOUCH_EVENT_TRACE_ENABLED {
            let _ = TOUCH_EVENT_TRACE_SAMPLES.try_send(event);
        }
        if let Err(command) = TOUCH_PIPELINE_CONTROL
            .interruptible(TOUCH_PIPELINE_EVENTS.send(event))
            .await
        {
            #[cfg(feature = "ui-interaction-trace")]
            crate::firmware::interaction_trace::record(event.trace_id, 6, event.time_ms(), 0, 0);
            // A stalled consumer must not prevent panel quiescence. Cancel the
            // interrupted gesture on resume, before accepting another raw frame.
            TOUCH_PIPELINE_CONTROL.hold_suspended(command, true).await;
            crate::firmware::acquisition_metrics::start_delivery_window(Instant::now().as_millis());
            request_touch_pipeline_reset();
            return;
        }
    }
    #[cfg(feature = "ui-interaction-trace")]
    trace.finish_batch(ends_gesture);
}

fn publish_imu_activity(event: TouchEvent) {
    TOUCH_IMU_ACTIVITY.signal(snapshot_for_event(event));
}

pub async fn push_touch_input_sample(frame: TouchSampleFrame) {
    TOUCH_PIPELINE_INPUTS
        .send(TouchPipelineInput::Sample(frame))
        .await;
}

pub fn request_touch_pipeline_reset() {
    crate::firmware::touch::admission::reset_requested();
    TOUCH_PIPELINE_RESET_PENDING.fetch_add(1, Ordering::Release);
    let _ = TOUCH_PIPELINE_INPUTS.try_send(TouchPipelineInput::Reset);
}

pub async fn reset_touch_pipeline() {
    request_touch_pipeline_reset();
}
