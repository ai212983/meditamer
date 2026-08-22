//! Cross-task wall-clock query result.
//!
//! The serial task remains the sole owner of RTC I2C access
//! ([`crate::firmware::types::InkplateRtcDriver`]); this type only carries
//! the outcome of one fresh, on-demand read across the
//! `WALL_CLOCK_REQUESTS`/`WALL_CLOCK_RESPONSES` channel pair to the display
//! task, for the Ambient Home screen (`docs/plans/ambient-home-prototype.md`).
//! No result is cached -- every request produces a fresh RTC transaction.

/// Outcome of one `WALL_CLOCK_REQUESTS` round trip.
#[derive(Clone, Copy, Debug)]
pub(crate) enum WallClockQueryResult {
    /// A fresh RTC read. `snapshot.valid` distinguishes a usable reading
    /// from a readable-but-unset/stopped clock.
    Snapshot(rtc::driver::WallClockSnapshot),
    /// The RTC I2C transaction itself failed.
    I2cError,
}
