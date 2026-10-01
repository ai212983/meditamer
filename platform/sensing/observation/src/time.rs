//! Explicit monotonic time.
//!
//! Every core decision in this crate takes `now` as a caller-supplied
//! [`Instant`] instead of reading a real clock, so subscription, demand, and
//! delivery behavior stay deterministic on host (`platform/connectivity/arbitration`'s
//! and `platform/connectivity/ble`'s pattern). Callers pick the tick unit; targets use
//! milliseconds from `embassy_time::Instant`, and host tests use whatever is
//! convenient for the scenario.

/// A point in monotonic time, in caller-chosen ticks (targets: milliseconds).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(pub u64);

impl Instant {
    pub const ZERO: Instant = Instant(0);

    pub const fn from_ticks(ticks: u64) -> Self {
        Instant(ticks)
    }

    /// Adds `duration`, saturating at `u64::MAX` rather than wrapping.
    pub fn saturating_add(self, duration: Duration) -> Instant {
        Instant(self.0.saturating_add(duration.0 as u64))
    }

    /// Adds `duration`, reporting overflow instead of wrapping or saturating.
    pub fn checked_add(self, duration: Duration) -> Option<Instant> {
        self.0.checked_add(duration.0 as u64).map(Instant)
    }

    /// The elapsed duration since `earlier`, clamped to `Duration::MAX` and
    /// returning `None` (rather than wrapping) if `earlier` is in the future.
    pub fn checked_duration_since(self, earlier: Instant) -> Option<Duration> {
        self.0
            .checked_sub(earlier.0)
            .map(|ticks| Duration(ticks.min(u32::MAX as u64) as u32))
    }
}

/// A span of caller-chosen ticks (targets: milliseconds), bounded to `u32` so
/// it stays a cheap `Copy` value in subscription and demand storage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration(pub u32);

impl Duration {
    pub const ZERO: Duration = Duration(0);
    pub const MAX: Duration = Duration(u32::MAX);

    pub const fn from_ticks(ticks: u32) -> Self {
        Duration(ticks)
    }

    pub const fn from_secs(seconds: u32) -> Self {
        Duration(seconds.saturating_mul(1000))
    }

    pub const fn from_millis(millis: u32) -> Self {
        Duration(millis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturating_add_clamps_instead_of_wrapping() {
        let near_max = Instant(u64::MAX - 1);
        assert_eq!(near_max.saturating_add(Duration(10)), Instant(u64::MAX));
    }

    #[test]
    fn checked_add_reports_overflow() {
        let near_max = Instant(u64::MAX - 1);
        assert_eq!(near_max.checked_add(Duration(10)), None);
        assert_eq!(Instant(5).checked_add(Duration(10)), Some(Instant(15)));
    }

    #[test]
    fn duration_since_is_none_when_earlier_is_in_the_future() {
        assert_eq!(Instant(5).checked_duration_since(Instant(10)), None);
        assert_eq!(
            Instant(10).checked_duration_since(Instant(5)),
            Some(Duration(5))
        );
    }
}
