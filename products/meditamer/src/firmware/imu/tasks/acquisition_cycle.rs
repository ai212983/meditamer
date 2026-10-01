use crate::firmware::types::i2c::ImuBusTiming;
use embassy_time::{Duration, Instant};
use inkplate_tempera::imu::ImuReadTiming;

use crate::firmware::{
    imu::{
        config::{IMU_INIT_RETRY_MS, TAP_TRACE_AUX_SAMPLE_MS, TAP_TRACE_ENABLED},
        metrics,
        scheduler::{AdaptiveImuScheduler, SamplingMode},
        timing,
        types::ImuSuppressionReason,
    },
    touch::types::{TouchActivitySnapshot, TouchStatus},
};

use crate::firmware::bounded_control::ControlRequest;

pub(super) enum SuppressionPlan {
    Hold,
    Proceed,
    Suppress(ImuSuppressionReason),
    Resume,
}

pub(super) struct SampleDurations {
    pub service_us: u32,
    pub publish_us: u32,
    pub read_timing: ImuReadTiming,
    pub bus_timing: ImuBusTiming,
    pub aux_us: u32,
}

pub(super) struct CycleState {
    pub last_mode: SamplingMode,
    pub touch: TouchActivitySnapshot,
    pub touch_initializing: bool,
    pub suppression: Option<ImuSuppressionReason>,
    pub ready: bool,
    pub fault_notified: bool,
    pub retry_at: Instant,
    pub next_sample_at: Instant,
    pub last_sample_at: Option<Instant>,
    pub pending_discontinuity: bool,
    pub power_good: i16,
    pub next_aux_at: Instant,
}

impl CycleState {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            last_mode: SamplingMode::Idle,
            touch: TouchActivitySnapshot::default(),
            touch_initializing: true,
            suppression: None,
            ready: false,
            fault_notified: false,
            retry_at: now,
            next_sample_at: now,
            last_sample_at: None,
            pending_discontinuity: true,
            power_good: -1,
            next_aux_at: now,
        }
    }

    pub(super) fn on_demand(&mut self, scheduler: &mut AdaptiveImuScheduler, active_until_ms: u64) {
        super::promote_scheduler(scheduler, &mut self.last_mode, active_until_ms);
        let active_deadline = Instant::now() + Duration::from_micros(scheduler.active_period_us());
        if active_deadline < self.next_sample_at {
            self.next_sample_at = active_deadline;
        }
    }

    pub(super) fn on_touch_snapshot(&mut self, snapshot: TouchActivitySnapshot) {
        self.touch = snapshot;
        self.next_sample_at = Instant::now();
    }

    pub(super) fn on_touch_status(&mut self, status: TouchStatus) {
        self.touch_initializing = matches!(status, TouchStatus::Initializing);
        self.next_sample_at = Instant::now();
    }

    // Synchronous: the caller acknowledges the command first, then records it,
    // so no nested future holds a borrow of the full state.
    pub(super) fn on_control_command(&mut self, _command: ControlRequest) {
        self.pending_discontinuity = true;
        self.next_sample_at = Instant::now();
    }

    pub(super) fn note_missed_deadline(
        &self,
        scheduler: &AdaptiveImuScheduler,
        now: Instant,
        now_ms: u64,
    ) {
        if now.as_micros()
            > self
                .next_sample_at
                .as_micros()
                .saturating_add(scheduler.period_us(now_ms))
        {
            metrics::record_missed_deadline();
        }
    }

    pub(super) fn refresh_mode(
        &mut self,
        scheduler: &AdaptiveImuScheduler,
        now_ms: u64,
    ) -> SamplingMode {
        let current_mode = scheduler.mode(now_ms);
        metrics::record_mode_change(self.last_mode, current_mode);
        self.last_mode = current_mode;
        current_mode
    }

    /// Touch-only suppression decision; the caller publishes the transition.
    pub(super) fn planned_suppression(&self, now_ms: u64) -> Option<ImuSuppressionReason> {
        if self.touch_initializing || super::touch_bus_quiet(self.touch, now_ms) {
            Some(ImuSuppressionReason::Touch)
        } else {
            None
        }
    }

    pub(super) fn suppression_plan(&self, now_ms: u64) -> SuppressionPlan {
        let planned = self.planned_suppression(now_ms);
        if planned != self.suppression {
            return match planned {
                Some(reason) => SuppressionPlan::Suppress(reason),
                None => SuppressionPlan::Resume,
            };
        }
        if self.suppression.is_some() {
            SuppressionPlan::Hold
        } else {
            SuppressionPlan::Proceed
        }
    }

    pub(super) fn commit_suppressed(&mut self, reason: ImuSuppressionReason) {
        self.pending_discontinuity = true;
        self.suppression = Some(reason);
    }

    pub(super) fn commit_resumed(
        &mut self,
        scheduler: &mut AdaptiveImuScheduler,
        active_hold_ms: u64,
        now_ms: u64,
    ) {
        super::promote_scheduler(
            scheduler,
            &mut self.last_mode,
            now_ms.saturating_add(active_hold_ms),
        );
        self.pending_discontinuity = true;
        self.suppression = None;
    }

    pub(super) fn hold_suppressed(&mut self, scheduler: &AdaptiveImuScheduler, now: Instant) {
        metrics::record_suppressed(self.suppression.unwrap_or(ImuSuppressionReason::Touch));
        self.next_sample_at = now + Duration::from_micros(scheduler.idle_period_us());
    }

    pub(super) fn init_due(&self, now: Instant) -> bool {
        !self.ready && now >= self.retry_at
    }

    pub(super) fn commit_init_success(
        &mut self,
        scheduler: &mut AdaptiveImuScheduler,
        active_hold_ms: u64,
        now_ms: u64,
    ) {
        self.ready = true;
        self.fault_notified = false;
        self.pending_discontinuity = true;
        super::promote_scheduler(
            scheduler,
            &mut self.last_mode,
            now_ms.saturating_add(active_hold_ms),
        );
    }

    pub(super) fn should_report_init_fault(&self) -> bool {
        !self.fault_notified
    }

    pub(super) fn commit_init_fault_reported(&mut self) {
        self.fault_notified = true;
    }

    pub(super) fn defer_retry(&mut self) {
        self.retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
    }

    pub(super) fn gap_discontinuity(&self, now: Instant, expected_period_us: u64) -> bool {
        self.last_sample_at.is_some_and(|last| {
            now.saturating_duration_since(last).as_micros() > expected_period_us.saturating_mul(2)
        })
    }

    pub(super) fn aux_due(&self, now: Instant) -> bool {
        TAP_TRACE_ENABLED && now >= self.next_aux_at
    }

    pub(super) fn commit_aux(&mut self, power_good: i16) {
        self.power_good = power_good;
        self.next_aux_at = Instant::now() + Duration::from_millis(TAP_TRACE_AUX_SAMPLE_MS);
    }

    pub(super) fn power_good_for_trace(&self, now: Instant) -> i16 {
        if now < self.next_aux_at {
            self.power_good
        } else {
            -1
        }
    }

    pub(super) fn sample_discontinuity(&self, gap_discontinuity: bool) -> bool {
        self.pending_discontinuity || gap_discontinuity
    }

    pub(super) fn commit_sample(
        &mut self,
        scheduler: &AdaptiveImuScheduler,
        now: Instant,
        now_ms: u64,
        durations: SampleDurations,
        gap_discontinuity: bool,
        rebase_deadline: &mut bool,
    ) {
        let active = scheduler.mode(now_ms) == SamplingMode::Active;
        timing::record(
            timing::Cycle {
                start_us: now.as_micros(),
                wake_us: now
                    .saturating_duration_since(self.next_sample_at)
                    .as_micros()
                    .min(u32::MAX as u64) as u32,
                service_us: durations.service_us,
                publish_us: durations.publish_us,
                read_timing: durations.read_timing,
                bus_timing: durations.bus_timing,
                aux_us: durations.aux_us,
                skipped_after: 0,
                rebase: false,
                resumed: false,
                mode_changed: false,
            },
            active,
            self.pending_discontinuity,
        );
        *rebase_deadline |= self.pending_discontinuity;
        crate::firmware::acquisition_metrics::IMU.sample(
            now_ms,
            active,
            self.pending_discontinuity,
        );
        metrics::record_sample(now_ms, scheduler.mode(now_ms));
        if self.pending_discontinuity || gap_discontinuity {
            metrics::record_discontinuity();
        }
        self.pending_discontinuity = false;
        self.last_sample_at = Some(now);
    }

    pub(super) fn commit_sample_failure(&mut self) {
        self.ready = false;
        self.fault_notified = true;
        self.pending_discontinuity = true;
        self.retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
    }

    pub(super) fn cycle_rebase(&self, rebase_deadline: bool) -> bool {
        rebase_deadline || !self.ready
    }

    pub(super) fn finish_cycle(
        &mut self,
        scheduler: &mut AdaptiveImuScheduler,
        current_mode: SamplingMode,
        rebase: bool,
    ) {
        let completed = Instant::now();
        let mode_changed = scheduler.mode(completed.as_millis()) != current_mode;
        let (next, skipped) = scheduler.next_deadline(
            self.next_sample_at.as_micros(),
            completed.as_micros(),
            current_mode,
            rebase,
        );
        timing::note_deadline(rebase, mode_changed);
        timing::skipped(skipped);
        if skipped > 0 {
            metrics::record_missed_deadline();
        }
        self.next_sample_at = Instant::from_micros(next);
    }
}
