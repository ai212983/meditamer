#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SamplingMode {
    Idle,
    Active,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AdaptiveImuScheduler {
    idle_period_us: u64,
    active_period_us: u64,
    active_until_ms: u64,
    catch_up_used: bool,
}

impl AdaptiveImuScheduler {
    pub(crate) const fn new(idle_hz: u16, active_hz: u16) -> Self {
        Self {
            idle_period_us: period_us(idle_hz),
            active_period_us: period_us(active_hz),
            active_until_ms: 0,
            catch_up_used: false,
        }
    }

    pub(crate) fn promote_until(&mut self, active_until_ms: u64) {
        self.active_until_ms = self.active_until_ms.max(active_until_ms);
    }

    pub(crate) fn mode(&self, now_ms: u64) -> SamplingMode {
        if now_ms < self.active_until_ms {
            SamplingMode::Active
        } else {
            SamplingMode::Idle
        }
    }

    pub(crate) fn period_us(&self, now_ms: u64) -> u64 {
        match self.mode(now_ms) {
            SamplingMode::Idle => self.idle_period_us,
            SamplingMode::Active => self.active_period_us,
        }
    }

    pub(crate) fn next_deadline(
        &mut self,
        due_us: u64,
        completed_us: u64,
        previous_mode: SamplingMode,
        rebase: bool,
    ) -> (u64, u64) {
        let now_ms = completed_us / 1000;
        let period = self.period_us(now_ms);
        let current = self.mode(now_ms);
        if rebase || current != previous_mode {
            if current == SamplingMode::Active {
                let slot = due_us.saturating_add(self.active_period_us);
                if slot > completed_us {
                    self.catch_up_used = false;
                    return (slot, 0);
                }
                if completed_us.saturating_sub(slot) <= self.active_period_us / 4 {
                    self.catch_up_used = true;
                    return (completed_us, 0);
                }
            }
            self.catch_up_used = false;
            return (completed_us.saturating_add(period), 0);
        }
        let next = due_us.saturating_add(period);
        if next > completed_us {
            self.catch_up_used = false;
            return (next, 0);
        }
        if current == SamplingMode::Active
            && !self.catch_up_used
            && completed_us < next.saturating_add(period)
            && completed_us.saturating_sub(next) <= period / 4
        {
            self.catch_up_used = true;
            return (completed_us, 0);
        }
        let skipped = (completed_us - next) / period + 1;
        (next.saturating_add(skipped.saturating_mul(period)), skipped)
    }

    pub(crate) const fn idle_period_us(&self) -> u64 {
        self.idle_period_us
    }

    pub(crate) const fn active_period_us(&self) -> u64 {
        self.active_period_us
    }
}

const fn period_us(hz: u16) -> u64 {
    1_000_000 / hz as u64
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn supports_requested_configurations() {
        let forty_eighty = AdaptiveImuScheduler::new(40, 80);
        assert_eq!(forty_eighty.idle_period_us(), 25_000);
        assert_eq!(forty_eighty.active_period_us(), 12_500);

        let hundred_one_twenty_five = AdaptiveImuScheduler::new(100, 125);
        assert_eq!(hundred_one_twenty_five.idle_period_us(), 10_000);
        assert_eq!(hundred_one_twenty_five.active_period_us(), 8_000);
    }

    #[test]
    fn promotion_extends_but_never_shortens_deadline() {
        let mut scheduler = AdaptiveImuScheduler::new(20, 125);
        assert_eq!(scheduler.mode(0), SamplingMode::Idle);
        scheduler.promote_until(1_000);
        scheduler.promote_until(500);
        assert_eq!(scheduler.mode(999), SamplingMode::Active);
        assert_eq!(scheduler.mode(1_000), SamplingMode::Idle);
    }
    #[test]
    fn work_time_does_not_shift_the_sampling_timeline() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 11000, SamplingMode::Active, false),
            (16000, 0)
        );
        assert_eq!(
            s.next_deadline(16000, 19000, SamplingMode::Active, false),
            (24000, 0)
        );
    }
    #[test]
    fn just_missed_slot_starts_immediately() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 17000, SamplingMode::Active, false),
            (17000, 0)
        );
    }
    #[test]
    fn second_consecutive_catch_up_is_suppressed() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 17000, SamplingMode::Active, false),
            (17000, 0)
        );
        assert_eq!(
            s.next_deadline(17000, 26000, SamplingMode::Active, false),
            (33000, 1)
        );
    }
    #[test]
    fn larger_overrun_skips_to_future() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 20000, SamplingMode::Active, false),
            (24000, 1)
        );
        assert_eq!(
            s.next_deadline(8000, 33000, SamplingMode::Active, false),
            (40000, 3)
        );
    }
    #[test]
    fn on_time_cycle_clears_catch_up_cap() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 17000, SamplingMode::Active, false),
            (17000, 0)
        );
        assert_eq!(
            s.next_deadline(17000, 20000, SamplingMode::Active, false),
            (25000, 0)
        );
        assert_eq!(
            s.next_deadline(25000, 34000, SamplingMode::Active, false),
            (34000, 0)
        );
    }
    #[test]
    fn rebase_clears_catch_up_cap() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 17000, SamplingMode::Active, false),
            (17000, 0)
        );
        assert_eq!(
            s.next_deadline(17000, 20000, SamplingMode::Active, true),
            (25000, 0)
        );
        assert_eq!(
            s.next_deadline(25000, 34000, SamplingMode::Active, false),
            (34000, 0)
        );
    }
    #[test]
    fn active_rebase_short_read_keeps_slot() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 11000, SamplingMode::Active, true),
            (16000, 0)
        );
    }
    #[test]
    fn active_rebase_just_over_slot_starts_immediately() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 17000, SamplingMode::Active, true),
            (17000, 0)
        );
        assert_eq!(
            s.next_deadline(8000, 18000, SamplingMode::Active, true),
            (18000, 0)
        );
    }
    #[test]
    fn large_active_rebase_overrun_rebases_from_completion() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 20000, SamplingMode::Active, true),
            (28000, 0)
        );
    }
    #[test]
    fn idle_rebase_keeps_completion_anchor() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        assert_eq!(
            s.next_deadline(8000, 11000, SamplingMode::Idle, true),
            (61000, 0)
        );
    }
    #[test]
    fn active_rebase_catch_up_sets_cap() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 17000, SamplingMode::Active, true),
            (17000, 0)
        );
        assert_eq!(
            s.next_deadline(17000, 26000, SamplingMode::Active, false),
            (33000, 1)
        );
    }
    #[test]
    fn lifecycle_and_rate_changes_rebase_without_false_misses() {
        let mut s = AdaptiveImuScheduler::new(20, 125);
        s.promote_until(1000);
        assert_eq!(
            s.next_deadline(8000, 500000, SamplingMode::Active, true),
            (508000, 0)
        );
        assert_eq!(
            s.next_deadline(990000, 1000000, SamplingMode::Active, false),
            (1050000, 0)
        );
        assert_eq!(
            s.next_deadline(8000, 11000, SamplingMode::Idle, false),
            (16000, 0)
        );
    }
}
