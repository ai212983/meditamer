#[cfg(feature = "cpu-load")]
use cpu_load::profile::TouchPhase;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_time::{Duration, Instant};
#[cfg(feature = "cpu-load")]
use embedded_hal_async::delay::DelayNs;

use crate::firmware::{
    config::APP_EVENTS,
    input::gpio36::{Gpio36Action, Gpio36Classifier},
    touch::{
        config::{TOUCH_IMU_STATUS, TOUCH_INIT_RETRY_MS},
        scheduling,
    },
    types::{
        AppEvent, Gpio36InputPin, InkplateTouchDriver, TouchRetryTimer, TouchSampleFrame,
        TouchStatus,
    },
};

use super::{
    receive_command, state::ContactSamplingState, try_receive_command, TouchAcquisitionCommand,
    DEFERRED_CONTROL,
};
use crate::firmware::touch::tasks::{push_touch_input_sample, request_touch_pipeline_reset};

const CLASSIFIER_PROBE_MS: u64 = 8;
const RELEASE_TRACE_ENABLED: bool = option_env!("MEDITAMER_TOUCH_RELEASE_TRACE").is_some();

#[cfg(feature = "cpu-load")]
struct TracedRetryDelay<'a> {
    timer: &'a mut TouchRetryTimer,
    wakes: [scheduling::RetryWakeTiming; 2],
    count: u8,
}

#[cfg(feature = "cpu-load")]
impl TracedRetryDelay<'_> {
    fn new(timer: &mut TouchRetryTimer) -> TracedRetryDelay<'_> {
        TracedRetryDelay {
            timer,
            wakes: [scheduling::RetryWakeTiming::default(); 2],
            count: 0,
        }
    }
}

#[cfg(feature = "cpu-load")]
impl DelayNs for TracedRetryDelay<'_> {
    async fn delay_ns(&mut self, ns: u32) {
        let (before_count, _) = cpu_load::touch_timer_irq_snapshot();
        let armed_us = Instant::now().as_micros() as u32;
        self.timer.delay_nanos_async(ns).await;
        let resumed_us = Instant::now().as_micros() as u32;
        let (after_count, irq_us) = cpu_load::touch_timer_irq_snapshot();
        let irq_events = after_count.wrapping_sub(before_count);
        let irq_offset = irq_us.wrapping_sub(armed_us);
        let valid =
            irq_events == 1 && irq_offset > 0 && irq_offset <= resumed_us.wrapping_sub(armed_us);
        if let Some(wake) = self.wakes.get_mut(self.count as usize) {
            *wake = scheduling::RetryWakeTiming {
                armed_us,
                irq_us: if valid { irq_us } else { 0 },
                resumed_us,
                irq_events,
                valid,
            };
        }
        self.count = self.count.saturating_add(1);
    }
}

pub(super) enum ProbeResult {
    Complete,
    Command(TouchAcquisitionCommand),
    Fault,
}

pub(super) enum AcquisitionWake {
    Command(TouchAcquisitionCommand),
    Asserted,
    RecoveryDue,
}

pub(super) async fn wait_for_assertion_or_recovery(
    gpio36: &mut Gpio36InputPin,
    retry_timer: &mut TouchRetryTimer,
    poll_interval_ms: u64,
) -> AcquisitionWake {
    if poll_interval_ms == 0 {
        return AcquisitionWake::RecoveryDue;
    }
    match select3(
        receive_command(),
        gpio36.wait_for_low(),
        retry_timer.delay_millis_async(poll_interval_ms as u32),
    )
    .await
    {
        Either3::First(command) => AcquisitionWake::Command(command),
        Either3::Second(_) => AcquisitionWake::Asserted,
        Either3::Third(_) => AcquisitionWake::RecoveryDue,
    }
}

pub(super) async fn probe_asserted_line(
    touch: &mut InkplateTouchDriver,
    retry_timer: &mut TouchRetryTimer,
    gpio36: &Gpio36InputPin,
    classifier: &mut Gpio36Classifier,
    contact_sampling: &mut ContactSamplingState,
) -> ProbeResult {
    loop {
        if let Some(command) = try_receive_command() {
            return ProbeResult::Command(command);
        }
        if read_and_publish(
            touch,
            retry_timer,
            Instant::now().as_millis(),
            "asserted",
            classifier,
            contact_sampling,
        )
        .await
        .is_err()
        {
            return ProbeResult::Fault;
        }
        if !classifier.is_pending() || !gpio36.is_low() {
            return ProbeResult::Complete;
        }
        // A held-low WAKE press has no touch report. Probe only while source
        // classification is pending; normal touch motion remains IRQ-driven.
        match select(
            receive_command(),
            retry_timer.delay_millis_async(CLASSIFIER_PROBE_MS as u32),
        )
        .await
        {
            Either::First(command) => return ProbeResult::Command(command),
            Either::Second(_) => {}
        }
    }
}

pub(super) async fn read_and_publish(
    touch: &mut InkplateTouchDriver,
    retry_timer: &mut TouchRetryTimer,
    t_ms: u64,
    phase: &str,
    classifier: &mut Gpio36Classifier,
    contact_sampling: &mut ContactSamplingState,
) -> Result<(), ()> {
    let admission_epoch = crate::firmware::touch::admission::epoch();
    crate::firmware::types::i2c::reset_touch();
    contact_sampling.record_read_start(t_ms);
    let read_started = Instant::now();
    #[cfg(feature = "cpu-load")]
    let mut observed_delay = TracedRetryDelay::new(retry_timer);
    #[cfg(feature = "cpu-load")]
    let delay = &mut observed_delay;
    #[cfg(not(feature = "cpu-load"))]
    let delay = retry_timer;
    let (sample, retry_timing) = touch
        .read_sample_timed_with_delay(0, delay)
        .await
        .map_err(|_| ())?;
    #[cfg(feature = "cpu-load")]
    let retry_wakes = observed_delay.wakes;
    #[cfg(not(feature = "cpu-load"))]
    let retry_wakes = [scheduling::RetryWakeTiming::default(); 2];
    #[cfg(feature = "cpu-load")]
    cpu_load::tail_note_touch_phase(TouchPhase::Timing);
    let read_timing = scheduling::TouchReadTiming {
        read_us: read_started.elapsed().as_micros().min(u32::MAX as u64) as u32,
        bus: crate::firmware::types::i2c::touch_snapshot(),
        waits: crate::firmware::types::i2c::touch_wait_snapshot(),
        retry_wait_us: retry_timing.wait_us,
        retries: retry_timing.retries,
        retry_wakes,
    };
    #[cfg(feature = "cpu-load")]
    cpu_load::tail_note_touch_phase(TouchPhase::Classifier);
    let authoritative_count =
        contact_sampling.classify_touch_count(&sample.raw, sample.touch_count);
    if RELEASE_TRACE_ENABLED && contact_sampling.should_trace(t_ms, authoritative_count) {
        console::println!(
            "TOUCH_RELEASE_TRACE phase={} t_ms={} authoritative={:?} decoded_count={} raw={:02x},{:02x},{:02x},{:02x},{:02x},{:02x},{:02x},{:02x}",
            phase,
            t_ms,
            authoritative_count,
            sample.touch_count,
            sample.raw[0],
            sample.raw[1],
            sample.raw[2],
            sample.raw[3],
            sample.raw[4],
            sample.raw[5],
            sample.raw[6],
            sample.raw[7],
        );
    }
    let action =
        classifier.observe_touch_probe(t_ms, authoritative_count.is_some_and(|count| count > 0));
    publish_classifier_action(&*classifier, action).await;
    #[cfg(feature = "cpu-load")]
    cpu_load::tail_note_touch_phase(TouchPhase::Metrics);
    if let Some(touch_count) = authoritative_count {
        contact_sampling.record_authoritative_count(touch_count);
        scheduling::record_sample(t_ms, touch_count);
        scheduling::record_read(t_ms, touch_count, read_timing);
        crate::firmware::acquisition_metrics::TOUCH.sample(t_ms, touch_count > 0, false);
        #[cfg(feature = "firmware-trace")]
        crate::firmware::trace::physical_touch_sample(t_ms, touch_count);
        #[cfg(feature = "ui-interaction-trace")]
        let trace_id = crate::firmware::interaction_trace::sample(
            t_ms,
            touch_count,
            sample.points[0].x,
            sample.points[0].y,
        );
        #[cfg(feature = "ui-interaction-trace")]
        let trace_frame_id = crate::firmware::touch::interaction_trace::next_frame_id();
        if !crate::firmware::touch::admission::observe(admission_epoch, touch_count) {
            #[cfg(feature = "ui-interaction-trace")]
            crate::firmware::interaction_trace::record(trace_id, 38, t_ms, admission_epoch, 2);
            #[cfg(feature = "cpu-load")]
            cpu_load::tail_note_touch_phase(TouchPhase::PostPublish);
            return Ok(());
        }
        let frame = TouchSampleFrame {
            admission_epoch,
            #[cfg(feature = "ui-interaction-trace")]
            trace_id,
            #[cfg(feature = "ui-interaction-trace")]
            trace_frame_id,
            t_ms,
            sample,
        };
        #[cfg(feature = "cpu-load")]
        cpu_load::tail_note_touch_phase(TouchPhase::FrameEnqueue);
        if !publish_with_control(push_touch_input_sample(frame)).await {
            request_touch_pipeline_reset();
        }
    }
    #[cfg(feature = "cpu-load")]
    cpu_load::tail_note_touch_phase(TouchPhase::PostPublish);
    Ok(())
}

pub(super) async fn handle_fault(
    touch: &mut InkplateTouchDriver,
    ready: &mut bool,
    retry_at: &mut Instant,
    classifier: &mut Gpio36Classifier,
) {
    crate::firmware::acquisition_metrics::TOUCH.pause();
    scheduling::pause();
    *ready = false;
    let _ = touch.shutdown().await;
    request_touch_pipeline_reset();
    *retry_at = Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS);
    // Neither a pending GPIO36 classification nor an already-accepted WAKE
    // press may survive a touch acquisition fault. A pending window's
    // timing is measured against real elapsed time, and this fault's own
    // downtime (shutdown, retry backoff) would otherwise count toward it,
    // so recovery could immediately classify a probe as `WakeButtonPressed`
    // that has nothing to do with the assertion that started the window. An
    // already-accepted press has the sharper problem: if it is still
    // physically held when the fault fires, ending ownership without
    // suppression would let the same hold be classified as a brand-new
    // press once probing resumes. `cancel_accepted` clears and suppresses
    // both; the suspend path uses the same call for the same reason.
    classifier.cancel_accepted();
    crate::firmware::input::gpio36::sync_wake_snapshot(classifier);
    publish_touch_status(TouchStatus::Fault).await;
    console::println!("touch: read_error; retrying");
}

static PENDING_STATUS: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    TouchStatus,
> = embassy_sync::signal::Signal::new();

pub(super) async fn publish_touch_status(status: TouchStatus) {
    TOUCH_IMU_STATUS.signal(status);
    // Status is latest state; never block initialization/resume acknowledgements
    // on the application queue. A newer state supersedes a deferred older one.
    if APP_EVENTS.try_send(AppEvent::TouchStatus(status)).is_err() {
        PENDING_STATUS.signal(status);
    } else {
        PENDING_STATUS.reset();
    }
}

pub(super) async fn flush_pending_status() {
    if let Some(status) = PENDING_STATUS.try_take() {
        if !publish_with_control(APP_EVENTS.send(AppEvent::TouchStatus(status))).await {
            PENDING_STATUS.signal(status);
        }
    }
}

/// Publishes a classified GPIO36 edge after mirroring the classifier's
/// acceptance into the cross-core snapshot. Snapshot first, edge second:
/// the display's staleness check can then never observe a newer edge with
/// an older snapshot. This publishes edges only; the long-press decision
/// stays on the display task.
pub(super) async fn publish_classifier_action(
    classifier: &Gpio36Classifier,
    action: Option<Gpio36Action>,
) {
    crate::firmware::input::gpio36::sync_wake_snapshot(classifier);
    publish_action(action).await;
}

pub(super) async fn publish_action(action: Option<Gpio36Action>) {
    if let Some(
        action @ (Gpio36Action::WakeButtonPressed { .. } | Gpio36Action::WakeButtonReleased { .. }),
    ) = action
    {
        #[cfg(feature = "firmware-trace")]
        if matches!(action, Gpio36Action::WakeButtonPressed { .. }) {
            crate::firmware::trace::trigger_wake(Instant::now().as_millis());
        }
        // Suspension cancels a pending/accepted WAKE classification. An action
        // that has not reached the UI when control arrives belongs to that boundary.
        let _ = publish_with_control(APP_EVENTS.send(AppEvent::Gpio36Action(action))).await;
    }
}

pub(super) async fn publish_with_control(output: impl core::future::Future<Output = ()>) -> bool {
    match select(receive_command(), output).await {
        Either::First(command) => {
            DEFERRED_CONTROL.signal(command);
            false
        }
        Either::Second(()) => true,
    }
}
