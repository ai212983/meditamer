//! Pure geometry and time math for Ambient Home.
//!
//! This module has no LVGL and no crate-path dependency: it only uses
//! `core`, so it can be pulled unmodified into a host test harness (see
//! `test-support/host/ui_shell_host_harness`) via `#[path]`. Every item uses
//! `pub` rather than a `pub(in crate::firmware::ui)` path restriction
//! for exactly that reason -- the restriction is resolved against whichever
//! crate the file is compiled into.

/// A point expressed as a fraction of the surface's width and height.
/// Components may fall outside `0.0..=1.0`; the default arc endpoints are
/// deliberately off-surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FractionalPoint {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

/// The same point resolved to absolute pixels for one concrete surface size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PixelPoint {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

pub(crate) fn to_pixels(point: FractionalPoint, width: f32, height: f32) -> PixelPoint {
    PixelPoint {
        x: point.x * width,
        y: point.y * height,
    }
}

/// The deliberately low-churn values shown on e-paper. Temperature is
/// rounded to tenths of a degree and humidity to a whole percent so sensor
/// noise does not cause a refresh when the visible text would be unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EnvironmentDisplay {
    pub(crate) temperature_tenths: i16,
    pub(crate) humidity_percent: u8,
}

pub(crate) fn environment_display(
    temperature_centidegrees: i16,
    humidity_millipercent: u32,
    temperature_offset_centidegrees: i16,
    humidity_scale_permille: u32,
) -> EnvironmentDisplay {
    let corrected_temperature =
        temperature_centidegrees.saturating_add(temperature_offset_centidegrees);
    let temperature_tenths = if corrected_temperature >= 0 {
        (i32::from(corrected_temperature) + 5) / 10
    } else {
        (i32::from(corrected_temperature) - 5) / 10
    };
    let calibrated_humidity = u64::from(humidity_millipercent)
        .saturating_mul(u64::from(humidity_scale_permille))
        .saturating_add(500)
        .saturating_div(1_000)
        .min(100_000) as u32;
    let humidity_percent = calibrated_humidity
        .saturating_add(500)
        .saturating_div(1_000)
        .min(100) as u8;
    EnvironmentDisplay {
        temperature_tenths: temperature_tenths as i16,
        humidity_percent,
    }
}

/// Corrected ambient temperature and humidity in raw sensor units, before
/// display rounding. Prefer the external ambient reading without applying
/// the onboard sensor's thermal correction; absence falls back to
/// corrected BME688 values. Shared by the footer and the mountain-snow
/// snow evaluation so all three see the same combined source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AmbientClimate {
    pub(crate) temperature_centidegrees: i16,
    pub(crate) humidity_millipercent: u32,
}

pub(crate) fn ambient_climate(
    onboard_temperature_centidegrees: i16,
    onboard_humidity_millipercent: u32,
    temperature_offset_centidegrees: i16,
    humidity_scale_permille: u32,
    external: Option<(i16, u32)>,
) -> AmbientClimate {
    if let Some((temperature, humidity)) = external {
        return AmbientClimate {
            temperature_centidegrees: temperature,
            humidity_millipercent: humidity,
        };
    }
    AmbientClimate {
        temperature_centidegrees: onboard_temperature_centidegrees
            .saturating_add(temperature_offset_centidegrees),
        humidity_millipercent: u64::from(onboard_humidity_millipercent)
            .saturating_mul(u64::from(humidity_scale_permille))
            .saturating_add(500)
            .saturating_div(1_000)
            .min(100_000) as u32,
    }
}

/// One complete Ambient Home configuration set. Validation falls back to the
/// complete default set when any field is invalid rather than mixing values
/// from different sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AmbientHomeConfig {
    pub(crate) arc_start: FractionalPoint,
    pub(crate) control_1: FractionalPoint,
    pub(crate) control_2: FractionalPoint,
    pub(crate) arc_end: FractionalPoint,
    /// Fraction of the surface's shorter dimension.
    pub(crate) circle_radius_fraction: f32,
    /// Local time-of-day, in seconds since local midnight, 0..86_400.
    pub(crate) start_seconds: u32,
    pub(crate) end_seconds: u32,
    pub(crate) update_period_seconds: u32,
    pub(crate) tap_to_show_time: bool,
}

impl AmbientHomeConfig {
    /// The sun path is 60 px above the former vector arc on the 600 px panel.
    /// Time spans 08:00-20:00 with five-minute updates and tap-to-show-time.
    pub(crate) const DEFAULT: Self = Self {
        arc_start: FractionalPoint {
            x: -0.08,
            y: 378.0 / 600.0,
        },
        control_1: FractionalPoint {
            x: 0.175,
            y: 15.0 / 600.0,
        },
        control_2: FractionalPoint {
            x: 0.825,
            y: 15.0 / 600.0,
        },
        arc_end: FractionalPoint {
            x: 1.08,
            y: 378.0 / 600.0,
        },
        circle_radius_fraction: 0.06,
        start_seconds: 8 * 3_600,
        end_seconds: 20 * 3_600,
        update_period_seconds: 5 * 60,
        tap_to_show_time: true,
    };

    fn is_valid(&self) -> bool {
        self.start_seconds < self.end_seconds
            && self.circle_radius_fraction > 0.0
            && self.update_period_seconds > 0
            && self.update_period_seconds <= self.end_seconds - self.start_seconds
    }

    /// Applies the configuration as one complete set: any invalid value
    /// selects the complete default set.
    pub(crate) fn validated(self) -> Self {
        if self.is_valid() {
            self
        } else {
            Self::DEFAULT
        }
    }
}

/// Seconds in a local day, used to derive "seconds since local midnight"
/// from an epoch and to detect the day boundary.
pub(crate) const SECONDS_PER_DAY: u32 = 86_400;

/// Local time-of-day, in seconds since local midnight, for a local epoch
/// timestamp. A new local day (crossing midnight) always yields a value
/// less than `config.start_seconds` (for any config produced by
/// [`AmbientHomeConfig::validated`], since `start_seconds < SECONDS_PER_DAY`
/// is implied by `start_seconds < end_seconds <= SECONDS_PER_DAY`-shaped
/// configs in practice), so [`journey_fraction`] naturally resets to the
/// pre-start position -- no separate day-rollover case is needed.
pub(crate) const fn local_seconds_of_day(local_epoch_seconds: u32) -> u32 {
    local_epoch_seconds % SECONDS_PER_DAY
}

/// The elapsed fraction of the configured time span. Before the start time
/// the circle rests at the arc
/// start (`0.0`); at and after the end time it rests at the arc end
/// (`1.0`); in between, the latest update boundary anchored at the start
/// time (in `update_period_seconds` steps) determines the position.
pub(crate) fn journey_fraction(config: &AmbientHomeConfig, local_seconds_of_day: u32) -> f32 {
    if local_seconds_of_day < config.start_seconds {
        return 0.0;
    }
    if local_seconds_of_day >= config.end_seconds {
        return 1.0;
    }
    let elapsed = local_seconds_of_day - config.start_seconds;
    let steps = elapsed / config.update_period_seconds;
    let boundary_seconds = config.start_seconds + steps * config.update_period_seconds;
    let span = (config.end_seconds - config.start_seconds) as f32;
    (boundary_seconds - config.start_seconds) as f32 / span
}

/// How many seconds until [`journey_fraction`] can next produce a different
/// result for this config, starting from `local_seconds_of_day`. Used to
/// throttle wall-clock polling instead of re-checking on every frame tick.
/// Always returns at least `1` so callers never schedule a zero-delay poll.
pub(crate) fn seconds_until_next_boundary(
    config: &AmbientHomeConfig,
    local_seconds_of_day: u32,
) -> u32 {
    let delta = if local_seconds_of_day < config.start_seconds {
        config.start_seconds - local_seconds_of_day
    } else if local_seconds_of_day >= config.end_seconds {
        // Nothing changes again until local midnight resets the day.
        SECONDS_PER_DAY - (local_seconds_of_day % SECONDS_PER_DAY)
    } else {
        let elapsed = local_seconds_of_day - config.start_seconds;
        let steps = elapsed / config.update_period_seconds;
        let next_boundary = (config.start_seconds + (steps + 1) * config.update_period_seconds)
            .min(config.end_seconds);
        next_boundary.saturating_sub(local_seconds_of_day)
    };
    delta.max(1)
}

/// A point on the cubic Bezier curve `p0..=p3` at parameter `t` (clamped to
/// `0.0..=1.0`), evaluated directly in pixel space.
pub(crate) fn point_on_curve(
    p0: PixelPoint,
    p1: PixelPoint,
    p2: PixelPoint,
    p3: PixelPoint,
    t: f32,
) -> PixelPoint {
    let t = t.clamp(0.0, 1.0);
    let mt = 1.0 - t;
    let a = mt * mt * mt;
    let b = 3.0 * mt * mt * t;
    let c = 3.0 * mt * t * t;
    let d = t * t * t;
    PixelPoint {
        x: a * p0.x + b * p1.x + c * p2.x + d * p3.x,
        y: a * p0.y + b * p1.y + c * p2.y + d * p3.y,
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        assert!(AmbientHomeConfig::DEFAULT.is_valid());
        assert_eq!(
            AmbientHomeConfig::DEFAULT.validated(),
            AmbientHomeConfig::DEFAULT
        );
    }

    #[test]
    fn combined_climate_prefers_external_reading_verbatim() {
        assert_eq!(
            ambient_climate(2_261, 44_999, -434, 1_445, Some((1_950, 50_000))),
            AmbientClimate {
                temperature_centidegrees: 1_950,
                humidity_millipercent: 50_000,
            }
        );
    }

    #[test]
    fn combined_climate_falls_back_to_corrected_onboard() {
        assert_eq!(
            ambient_climate(2_261, 44_999, -434, 1_445, None),
            AmbientClimate {
                temperature_centidegrees: 1_827,
                humidity_millipercent: 65_024,
            }
        );
        // Onboard extremes saturate exactly as the inline correction did.
        assert_eq!(
            ambient_climate(i16::MAX, u32::MAX, 100, 2_000, None).temperature_centidegrees,
            i16::MAX
        );
    }

    #[test]
    fn combined_climate_matches_inline_display_correction() {
        // Footer equivalence: selecting first, then displaying identity,
        // must equal the old display-with-correction in one step.
        let cases = [
            (2_261, 44_999, -434, 1_445, None),
            (-456, 100_900, -434, 1_445, None),
            (0, 0, -434, 1_445, None),
            (2_261, 44_999, -434, 1_445, Some((1_950, 50_000))),
        ];
        for (temp, hum, offset, scale, external) in cases {
            let climate = ambient_climate(temp, hum, offset, scale, external);
            let (raw_temp, raw_hum, raw_offset, raw_scale) = match external {
                Some((t, h)) => (t, h, 0, 1_000),
                None => (temp, hum, offset, scale),
            };
            assert_eq!(
                environment_display(
                    climate.temperature_centidegrees,
                    climate.humidity_millipercent,
                    0,
                    1_000
                ),
                environment_display(raw_temp, raw_hum, raw_offset, raw_scale),
            );
        }
    }

    #[test]
    fn environmental_values_are_rounded_for_stable_epaper_text() {
        assert_eq!(
            environment_display(2_261, 44_999, 0, 1_000),
            EnvironmentDisplay {
                temperature_tenths: 226,
                humidity_percent: 45,
            }
        );
        assert_eq!(
            environment_display(-456, 100_900, 0, 1_000).temperature_tenths,
            -46
        );
        assert_eq!(
            environment_display(-456, 100_900, 0, 1_000).humidity_percent,
            100
        );
    }

    #[test]
    fn inkplate_calibration_applies_temperature_offset_and_humidity_gain() {
        assert_eq!(
            environment_display(3_066, 28_569, -434, 1_445),
            EnvironmentDisplay {
                temperature_tenths: 263,
                humidity_percent: 41,
            }
        );
    }

    #[test]
    fn invalid_config_falls_back_to_the_complete_default_set() {
        let mut broken = AmbientHomeConfig::DEFAULT;
        broken.start_seconds = broken.end_seconds; // start must precede end
        broken.circle_radius_fraction = 42.0; // otherwise-valid field, irrelevant once one is broken
        assert_eq!(broken.validated(), AmbientHomeConfig::DEFAULT);
    }

    #[test]
    fn update_period_exceeding_span_is_invalid() {
        let mut broken = AmbientHomeConfig::DEFAULT;
        broken.update_period_seconds = broken.end_seconds - broken.start_seconds + 1;
        assert_eq!(broken.validated(), AmbientHomeConfig::DEFAULT);
    }

    #[test]
    fn before_start_rests_at_arc_start() {
        let config = AmbientHomeConfig::DEFAULT;
        assert_eq!(journey_fraction(&config, 0), 0.0);
        assert_eq!(journey_fraction(&config, config.start_seconds - 1), 0.0);
    }

    #[test]
    fn at_and_after_end_rests_at_arc_end() {
        let config = AmbientHomeConfig::DEFAULT;
        assert_eq!(journey_fraction(&config, config.end_seconds), 1.0);
        assert_eq!(journey_fraction(&config, config.end_seconds + 3_600), 1.0);
    }

    /// With the defaults, `14:04:59` shows
    /// the `14:00` position, while `14:05:00` shows the `14:05` position.
    #[test]
    fn update_boundaries_are_anchored_at_the_start_time() {
        let config = AmbientHomeConfig::DEFAULT;
        let span = (config.end_seconds - config.start_seconds) as f32;

        let at_14_00 = 14 * 3_600;
        let at_14_04_59 = at_14_00 + 4 * 60 + 59;
        let at_14_05_00 = at_14_00 + 5 * 60;

        let fraction_14_00 = (at_14_00 - config.start_seconds as i32) as f32 / span;
        assert_eq!(
            journey_fraction(&config, at_14_04_59 as u32),
            fraction_14_00
        );
        assert_ne!(
            journey_fraction(&config, at_14_05_00 as u32),
            fraction_14_00
        );

        let fraction_14_05 = (at_14_05_00 - config.start_seconds as i32) as f32 / span;
        assert_eq!(
            journey_fraction(&config, at_14_05_00 as u32),
            fraction_14_05
        );
    }

    #[test]
    fn crossing_local_midnight_resets_to_the_pre_start_position() {
        let config = AmbientHomeConfig::DEFAULT;
        // 23:59:00 local, one minute before midnight: still resting at the end.
        assert_eq!(journey_fraction(&config, 23 * 3_600 + 59 * 60), 1.0);
        // The new day's epoch wraps seconds-of-day back below the start time.
        assert_eq!(local_seconds_of_day(SECONDS_PER_DAY + 60), 60);
        assert_eq!(
            journey_fraction(&config, local_seconds_of_day(SECONDS_PER_DAY + 60)),
            0.0
        );
    }

    #[test]
    fn end_time_is_an_additional_boundary_when_the_cadence_does_not_land_on_it() {
        let mut config = AmbientHomeConfig::DEFAULT;
        // A period that does not evenly divide the span: the last regular
        // grid boundary before the end is short of it, but the position
        // must still reach exactly 1.0 the instant the end time arrives.
        config.update_period_seconds = 7 * 60;
        let config = config.validated();
        assert_eq!(journey_fraction(&config, config.end_seconds - 1), {
            let span = (config.end_seconds - config.start_seconds) as f32;
            let elapsed = config.end_seconds - 1 - config.start_seconds;
            let steps = elapsed / config.update_period_seconds;
            let boundary = config.start_seconds + steps * config.update_period_seconds;
            (boundary - config.start_seconds) as f32 / span
        });
        assert_eq!(journey_fraction(&config, config.end_seconds), 1.0);
    }

    #[test]
    fn curve_endpoints_match_control_points_exactly() {
        let p0 = PixelPoint { x: -48.0, y: 438.0 };
        let p1 = PixelPoint { x: 105.0, y: 75.0 };
        let p2 = PixelPoint { x: 495.0, y: 75.0 };
        let p3 = PixelPoint { x: 648.0, y: 438.0 };
        assert_eq!(point_on_curve(p0, p1, p2, p3, 0.0), p0);
        assert_eq!(point_on_curve(p0, p1, p2, p3, 1.0), p3);
    }

    #[test]
    fn symmetric_default_arch_peaks_at_the_horizontal_midpoint() {
        let width = 600.0;
        let height = 600.0;
        let config = AmbientHomeConfig::DEFAULT;
        let p0 = to_pixels(config.arc_start, width, height);
        let p1 = to_pixels(config.control_1, width, height);
        let p2 = to_pixels(config.control_2, width, height);
        let p3 = to_pixels(config.arc_end, width, height);
        let midpoint = point_on_curve(p0, p1, p2, p3, 0.5);
        assert!((midpoint.x - width / 2.0).abs() < 0.01);
        assert!((midpoint.y - 105.75).abs() < 0.01);
    }

    #[test]
    fn next_boundary_never_returns_zero() {
        let config = AmbientHomeConfig::DEFAULT;
        assert!(seconds_until_next_boundary(&config, config.end_seconds) >= 1);
        assert!(seconds_until_next_boundary(&config, config.start_seconds) >= 1);
    }
}
