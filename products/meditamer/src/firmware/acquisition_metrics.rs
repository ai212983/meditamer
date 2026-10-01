//! Active acquisition gaps, separate from boot, fault, and suspension totals.
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

pub(crate) struct Samples {
    last: AtomicU32,
    started: AtomicBool,
    count: AtomicU32,
    max: AtomicU32,
    excluded: AtomicU32,
}
impl Samples {
    const fn new() -> Self {
        Self {
            last: AtomicU32::new(0),
            started: AtomicBool::new(false),
            count: AtomicU32::new(0),
            max: AtomicU32::new(0),
            excluded: AtomicU32::new(0),
        }
    }
    pub(crate) fn pause(&self) {
        self.started.store(false, Relaxed);
    }
    pub(crate) fn sample(&self, now_ms: u64, active: bool, discontinuity: bool) {
        if !active {
            self.pause();
            return;
        }
        let now = now_ms as u32;
        let previous = self.last.swap(now, Relaxed);
        if !self.started.swap(true, Relaxed) || discontinuity {
            self.excluded.fetch_add(1, Relaxed);
            return;
        }
        // Do not exclude an observed large gap: discontinuity is explicit
        // suspend/recovery policy, never inferred from this measurement.
        let gap = now.wrapping_sub(previous);
        self.count.fetch_add(1, Relaxed);
        self.max.fetch_max(gap, Relaxed);
    }
    pub(crate) fn snapshot(&self) -> [u32; 3] {
        [&self.count, &self.max, &self.excluded].map(|v| v.load(Relaxed))
    }
}
pub(crate) static TOUCH: Samples = Samples::new();
pub(crate) static IMU: Samples = Samples::new();
static DELIVERY_START: AtomicU32 = AtomicU32::new(0);
static DELIVERY_COUNT: AtomicU32 = AtomicU32::new(0);
static DELIVERY_MAX: AtomicU32 = AtomicU32::new(0);
static DELIVERY_EXCLUDED: AtomicU32 = AtomicU32::new(0);

pub(crate) fn start_delivery_window(now_ms: u64) {
    DELIVERY_START.store(now_ms as u32, Relaxed);
}
pub(crate) fn record_delivery(sample_ms: u64, now_ms: u64) {
    let sample = sample_ms as u32;
    // Frames acquired before the latest acknowledged resume belong to an
    // intentional suspension. Keep their excluded count visible.
    if (sample.wrapping_sub(DELIVERY_START.load(Relaxed)) as i32) < 0 {
        DELIVERY_EXCLUDED.fetch_add(1, Relaxed);
        return;
    }
    let delay = (now_ms as u32).wrapping_sub(sample);
    DELIVERY_COUNT.fetch_add(1, Relaxed);
    DELIVERY_MAX.fetch_max(delay, Relaxed);
}
pub(crate) fn delivery_snapshot() -> [u32; 3] {
    [&DELIVERY_COUNT, &DELIVERY_MAX, &DELIVERY_EXCLUDED].map(|v| v.load(Relaxed))
}

pub(crate) fn reset(now_ms: u64) {
    for sample in [&TOUCH, &IMU] {
        sample.pause();
        for counter in [&sample.count, &sample.max, &sample.excluded] {
            counter.store(0, Relaxed);
        }
    }
    start_delivery_window(now_ms);
    for counter in [&DELIVERY_COUNT, &DELIVERY_MAX, &DELIVERY_EXCLUDED] {
        counter.store(0, Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_pauses_are_excluded_but_late_healthy_samples_are_not() {
        let samples = Samples::new();
        samples.sample(u32::MAX as u64 - 7, true, false);
        samples.sample(u32::MAX as u64 + 1, true, false);
        samples.sample(u32::MAX as u64 + 40, true, false);
        assert_eq!(samples.snapshot(), [2, 39, 1]);
        let other = Samples::new();
        other.sample(100, true, false);
        other.sample(108, true, false);
        other.pause();
        other.sample(900, true, false);
        other.sample(924, true, false);
        other.sample(2000, true, true);
        assert_eq!(other.snapshot(), [2, 24, 3]);
        start_delivery_window(100);
        record_delivery(99, 1000);
        record_delivery(100, 125);
        assert_eq!(delivery_snapshot(), [1, 25, 1]);
        reset(1000);
        assert_eq!(delivery_snapshot(), [0, 0, 0]);
        TOUCH.sample(1000, true, false);
        TOUCH.sample(1008, true, false);
        assert_eq!(TOUCH.snapshot(), [1, 8, 1]);
        IMU.sample(1000, true, false);
        IMU.sample(1020, true, false);
        assert_eq!(IMU.snapshot(), [1, 20, 1]);
        reset(2000);
        assert_eq!(TOUCH.snapshot(), [0, 0, 0]);
        assert_eq!(IMU.snapshot(), [0, 0, 0]);
    }
}
