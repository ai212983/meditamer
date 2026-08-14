#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SamplingMode {
    Idle,
    Active,
}

#[derive(Clone, Copy, Debug)]
pub struct AdaptiveImuScheduler {
    idle_period_us: u64,
    active_period_us: u64,
    active_until_ms: u64,
}

impl AdaptiveImuScheduler {
    pub const fn new(idle_hz: u16, active_hz: u16) -> Self {
        Self {
            idle_period_us: period_us(idle_hz),
            active_period_us: period_us(active_hz),
            active_until_ms: 0,
        }
    }

    pub fn promote_until(&mut self, active_until_ms: u64) {
        self.active_until_ms = self.active_until_ms.max(active_until_ms);
    }

    pub fn mode(&self, now_ms: u64) -> SamplingMode {
        if now_ms < self.active_until_ms {
            SamplingMode::Active
        } else {
            SamplingMode::Idle
        }
    }

    pub fn period_us(&self, now_ms: u64) -> u64 {
        match self.mode(now_ms) {
            SamplingMode::Idle => self.idle_period_us,
            SamplingMode::Active => self.active_period_us,
        }
    }

    pub const fn idle_period_us(&self) -> u64 {
        self.idle_period_us
    }

    pub const fn active_period_us(&self) -> u64 {
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
}
