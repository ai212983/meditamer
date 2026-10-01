#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerServiceMetrics {
    last_handler_us: u64,
    max_gap_us: u64,
    max_runtime_us: u64,
}

impl TimerServiceMetrics {
    pub const fn new(now_us: u64) -> Self {
        Self {
            last_handler_us: now_us,
            max_gap_us: 0,
            max_runtime_us: 0,
        }
    }

    pub fn begin_handler(&mut self, now_us: u64) {
        self.max_gap_us = self
            .max_gap_us
            .max(now_us.saturating_sub(self.last_handler_us));
        self.last_handler_us = now_us;
    }

    pub fn finish_handler(&mut self, runtime_us: u64) {
        self.max_runtime_us = self.max_runtime_us.max(runtime_us);
    }

    pub const fn max_gap_us(&self) -> u64 {
        self.max_gap_us
    }

    pub const fn max_runtime_us(&self) -> u64 {
        self.max_runtime_us
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn timer_gap_and_runtime_maxima_are_independent_and_monotonic() {
        let mut metrics = TimerServiceMetrics::new(1_000);
        metrics.begin_handler(9_000);
        metrics.finish_handler(300);
        metrics.begin_handler(2_509_000);
        metrics.finish_handler(40);
        metrics.begin_handler(2_517_000);
        metrics.finish_handler(900);

        assert_eq!(metrics.max_gap_us(), 2_500_000);
        assert_eq!(metrics.max_runtime_us(), 900);
    }
}
