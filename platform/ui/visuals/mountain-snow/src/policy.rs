//! Temperature-to-snow policy for Ambient Home.
//!
//! The authored mountain has 20% natural snow at the 22 C work target.
//! Colder air builds snow to 100% at 18 C; warmer air melts it to 0% at
//! 24 C. The two straight segments are intentionally asymmetric so all
//! three product anchors are exact. Temperature is the selected, corrected
//! ambient sensor reading in centidegrees, before footer rounding.
//!
//! Evaluated only on the parent Ambient Home update tick. Small changes
//! around the last rendered reading and changes of less than two coverage
//! points do not repaint, so the mountain never starts its own refresh.

/// Policy tuning. Comparisons are against the last *rendered* input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    /// Minimum temperature movement from the last rendered reading.
    pub temperature_hysteresis_centidegrees: i16,
    /// Minimum coverage movement for a repaint, in percentage points.
    pub min_visible_step_percent: u8,
}

impl Config {
    pub const DEFAULT: Self = Self {
        temperature_hysteresis_centidegrees: 10,
        min_visible_step_percent: 2,
    };
}

/// Last rendered output. The screen can restore a copy if publication fails.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct State {
    rendered_percent: Option<u8>,
    rendered_temperature_centidegrees: Option<i16>,
}

impl State {
    #[must_use]
    pub fn rendered_percent(&self) -> Option<u8> {
        self.rendered_percent
    }

    #[must_use]
    pub fn rendered_temperature_centidegrees(&self) -> Option<i16> {
        self.rendered_temperature_centidegrees
    }
}

/// Piecewise linear coverage, rounded to the nearest whole percentage.
/// Endpoints saturate rather than extrapolate beyond the authored images.
#[must_use]
pub fn snow_percent(temperature_centidegrees: i16) -> u8 {
    let temperature = i32::from(temperature_centidegrees);
    match temperature {
        ..=1_800 => 100,
        2_400.. => 0,
        1_801..=2_200 => (20 + (2_200 - temperature + 2) / 5) as u8,
        _ => ((2_400 - temperature + 5) / 10) as u8,
    }
}

/// Evaluate one parent-tick temperature. Commits only when it returns a
/// new coverage value; the caller restores the prior state on a failed
/// canvas publication.
pub fn poll(config: &Config, state: &mut State, temperature_centidegrees: i16) -> Option<u8> {
    if config.temperature_hysteresis_centidegrees < 0 {
        return None;
    }
    let percent = snow_percent(temperature_centidegrees);
    if let (Some(last_percent), Some(last_temperature)) = (
        state.rendered_percent,
        state.rendered_temperature_centidegrees,
    ) {
        if percent == last_percent {
            return None;
        }
        // The three authored anchors must be exact even when the final
        // movement is only one point; elsewhere retain the repaint guard.
        let at_anchor = temperature_centidegrees <= 1_800
            || temperature_centidegrees == 2_200
            || temperature_centidegrees >= 2_400;
        if !at_anchor
            && (i32::from(temperature_centidegrees).abs_diff(i32::from(last_temperature))
                < config.temperature_hysteresis_centidegrees as u32
                || percent.abs_diff(last_percent) < config.min_visible_step_percent)
        {
            return None;
        }
    }
    state.rendered_percent = Some(percent);
    state.rendered_temperature_centidegrees = Some(temperature_centidegrees);
    Some(percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_product_anchors_and_saturation() {
        assert_eq!(snow_percent(1_700), 100);
        assert_eq!(snow_percent(1_800), 100);
        assert_eq!(snow_percent(2_000), 60);
        assert_eq!(snow_percent(2_200), 20);
        assert_eq!(snow_percent(2_300), 10);
        assert_eq!(snow_percent(2_400), 0);
        assert_eq!(snow_percent(2_500), 0);
    }

    #[test]
    fn coverage_is_monotone_through_both_segments() {
        let mut previous = 100;
        for temperature in 1_800..=2_400 {
            let current = snow_percent(temperature);
            assert!(current <= previous);
            previous = current;
        }
        assert_eq!(previous, 0);
    }

    #[test]
    fn repaint_waits_for_hysteresis_and_visible_step_from_last_render() {
        let mut state = State::default();
        assert_eq!(poll(&Config::DEFAULT, &mut state, 2_200), Some(20));
        assert_eq!(poll(&Config::DEFAULT, &mut state, 2_190), Some(22));
        assert_eq!(poll(&Config::DEFAULT, &mut state, 2_185), None);
        assert_eq!(state.rendered_temperature_centidegrees(), Some(2_190));
        assert_eq!(poll(&Config::DEFAULT, &mut state, 2_180), Some(24));
        assert_eq!(poll(&Config::DEFAULT, &mut state, 2_200), Some(20));
        assert_eq!(poll(&Config::DEFAULT, &mut state, 2_210), None);
        assert_eq!(poll(&Config::DEFAULT, &mut state, 2_220), Some(18));
    }

    #[test]
    fn first_sample_paints_even_at_an_endpoint() {
        let mut state = State::default();
        assert_eq!(poll(&Config::DEFAULT, &mut state, 1_800), Some(100));
        let mut state = State::default();
        assert_eq!(poll(&Config::DEFAULT, &mut state, 2_400), Some(0));
    }

    #[test]
    fn anchors_land_after_a_one_point_approach() {
        for (before, anchor, before_percent, anchor_percent) in [
            (1_805, 1_800, 99, 100),
            (2_195, 2_200, 21, 20),
            (2_390, 2_400, 1, 0),
        ] {
            let mut state = State::default();
            assert_eq!(
                poll(&Config::DEFAULT, &mut state, before),
                Some(before_percent)
            );
            assert_eq!(
                poll(&Config::DEFAULT, &mut state, anchor),
                Some(anchor_percent)
            );
            assert_eq!(poll(&Config::DEFAULT, &mut state, anchor), None);
        }
    }
}
