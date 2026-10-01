//! Product cadence and environment calibration.
//!
//! Target task wiring consumes these values without owning the policy.

/// Sample cadence on the normal Home screen. Room temperature and humidity move
/// slowly, and every sample costs a wakeup, a 50ms settle, a 20ms conversion
/// and a screen redraw. Exact cadence does not matter here; battery life
/// does.
pub const SAMPLE_INTERVAL_S: u64 = 60;

/// SHTC3 board/enclosure temperature correction in millidegrees, measured
/// against an external SHT45 at approximately 7 cm after thermal settling.
/// Kept here, not in the sensor driver, because it describes this board and
/// enclosure rather than the sensor itself.
pub const SELF_HEATING_MC: i32 = 1_630;

/// SHTC3 relative-humidity gain measured in the same settled SHT45 series.
/// 1000 means no correction.
pub const HUMIDITY_SCALE_PERMILLE: i32 = 1_169;
