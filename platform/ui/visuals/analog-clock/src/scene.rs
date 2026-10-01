//! Scene parameters: layout, time angles, light, hand heights, and finish.
//!
//! Units are dial radii unless noted: the dial is the unit disk around
//! (`center_x`, `center_y`) in output pixels, y down, z up. Angles are
//! radians clockwise from 12. Every numeric input is clamped by
//! [`ClockScene::clamped`] so host UI ranges and device callers share one
//! bound.

/// Sprite dimensions for pivot validation, shared with the host tool so
/// overrides are range-checked against the same bounds.
pub const HOUR_SPRITE: (u16, u16) = (245, 810);
pub const MINUTE_SPRITE: (u16, u16) = (156, 1014);

/// Hard cap on area-light samples. Twenty-four golden-angle taps already
/// resolve a soft penumbra; more buys noise, not quality, and costs linear
/// time in the shadow loop.
pub const MAX_SAMPLES: u8 = 24;

/// Full picture description. `width`/`height` are output pixels; the rest
/// is dial units or radians.
#[derive(Clone, Copy, Debug)]
pub struct ClockScene<'a> {
    /// Borrowed grayscale dial artwork. `None` (the default from
    /// [`ClockScene::for_size`]) keeps the procedural dial; `Some` with a
    /// valid [`crate::DialMap`] replaces paper, ticks, and hub with the
    /// baked image through the same light/shadow pipeline. An invalid map
    /// is clamped back to `None` (procedural fallback, never a panic).
    pub dial: Option<crate::DialMap<'a>>,
    pub width: u32,
    pub height: u32,
    pub center_x: f32,
    pub center_y: f32,
    pub radius: f32,
    pub hour_angle: f32,
    pub minute_angle: f32,
    /// Tip distance from the dial center, dial units.
    pub hour_len: f32,
    pub minute_len: f32,
    /// Light position: x/y in dial units from the center, z up.
    pub light_x: f32,
    pub light_y: f32,
    pub light_h: f32,
    /// Area-light disk radius, dial units. Zero is a point light.
    pub light_size: f32,
    /// Virtual hand heights above the dial plane, dial units.
    pub hour_h: f32,
    pub minute_h: f32,
    /// Independent additive specular gain and Blinn-Phong exponent.
    /// `spec_strength` is the global multiplier; `hour_spec`/`minute_spec`
    /// are per-hand multipliers (default 1) so the two exported specular
    /// maps can be matched without touching source.
    pub spec_strength: f32,
    pub hour_spec: f32,
    pub minute_spec: f32,
    pub shininess: f32,
    /// Hand shade attenuation, 0..1. Each hand's linear-light RGB shade
    /// is scaled by `(1 - hand_darkness)` before alpha compositing; 0 is
    /// the original look, 1 is black. Dial, shadows, alpha, and region
    /// masks are untouched.
    pub hand_darkness: f32,
    /// Unshadowed diffuse floor, 0..1 of albedo.
    pub ambient: f32,
    /// Area-light taps, clamped to 1..=[`MAX_SAMPLES`].
    pub samples: u8,
}

impl<'a> ClockScene<'a> {
    /// Centered layout for a `width` x `height` output with a neutral studio
    /// setup: light upper-left, minute hand above hour, soft penumbra.
    pub fn for_size(width: u32, height: u32) -> Self {
        let short = core::cmp::min(width, height) as f32;
        Self {
            dial: None,
            width,
            height,
            center_x: width as f32 / 2.0,
            center_y: height as f32 / 2.0,
            radius: 0.48 * short,
            hour_angle: 0.0,
            minute_angle: 0.0,
            hour_len: 0.55,
            minute_len: 0.85,
            light_x: 0.45,
            light_y: -0.35,
            light_h: 1.2,
            light_size: 0.12,
            hour_h: 0.06,
            minute_h: 0.12,
            spec_strength: 1.0,
            hour_spec: 1.0,
            minute_spec: 1.0,
            shininess: 24.0,
            hand_darkness: 0.0,
            ambient: 0.25,
            samples: 8,
        }
    }

    /// Copies the scene with every range enforced: non-negative sizes and
    /// heights, shininess >= 1, ambient 0..1, samples 1..=[`MAX_SAMPLES`],
    /// and a minimum light height so the light never sits in the plane.
    /// NaN becomes the neutral default for that field.
    pub fn clamped(&self) -> Self {
        let mut out = *self;
        let sane = |v: f32, fallback: f32| {
            if v.is_finite() {
                v
            } else {
                fallback
            }
        };
        let def = Self::for_size(self.width.max(1), self.height.max(1));
        out.center_x = sane(self.center_x, def.center_x);
        out.center_y = sane(self.center_y, def.center_y);
        out.radius = sane(self.radius, def.radius).max(1.0);
        out.hour_angle = sane(self.hour_angle, 0.0);
        out.minute_angle = sane(self.minute_angle, 0.0);
        out.hour_len = sane(self.hour_len, def.hour_len).clamp(0.05, 1.0);
        out.minute_len = sane(self.minute_len, def.minute_len).clamp(0.05, 1.0);
        out.light_x = sane(self.light_x, def.light_x).clamp(-2.0, 2.0);
        out.light_y = sane(self.light_y, def.light_y).clamp(-2.0, 2.0);
        out.light_h = sane(self.light_h, def.light_h).clamp(0.05, 4.0);
        out.light_size = sane(self.light_size, def.light_size).clamp(0.0, 0.6);
        out.hour_h = sane(self.hour_h, def.hour_h).clamp(0.0, 0.5);
        out.minute_h = sane(self.minute_h, def.minute_h).clamp(0.0, 0.5);
        out.spec_strength = sane(self.spec_strength, 1.0).clamp(0.0, 4.0);
        out.hour_spec = sane(self.hour_spec, 1.0).clamp(0.0, 4.0);
        out.minute_spec = sane(self.minute_spec, 1.0).clamp(0.0, 4.0);
        out.shininess = sane(self.shininess, 24.0).clamp(1.0, 256.0);
        out.hand_darkness = sane(self.hand_darkness, 0.0).clamp(0.0, 1.0);
        out.ambient = sane(self.ambient, 0.25).clamp(0.0, 1.0);
        out.samples = self.samples.clamp(1, MAX_SAMPLES);
        if !out.dial.is_some_and(|d| d.validate()) {
            out.dial = None;
        }
        out
    }
}

/// Clockwise-from-12 angles for a wall time. The hour hand advances with
/// the minutes; seconds do not move either hand in this prototype.
pub fn angles_for_time(hours: u8, minutes: u8, _seconds: u8) -> (f32, f32) {
    const TAU: f32 = core::f32::consts::TAU;
    let h = f32::from(hours % 12) + f32::from(minutes % 60) / 60.0;
    let m = f32::from(minutes % 60);
    (h / 12.0 * TAU, m / 60.0 * TAU)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn midnight_points_both_hands_up() {
        let (h, m) = angles_for_time(12, 0, 0);
        assert!(approx(h, 0.0) && approx(m, 0.0));
    }

    #[test]
    fn three_oclock_is_a_quarter_turn() {
        let (h, _) = angles_for_time(3, 0, 0);
        assert!(approx(h, core::f32::consts::FRAC_PI_2));
    }

    #[test]
    fn hour_hand_advances_with_minutes() {
        let (at_half, _) = angles_for_time(6, 30, 0);
        let (on_hour, _) = angles_for_time(6, 0, 0);
        let step = core::f32::consts::TAU / 12.0;
        assert!(approx(at_half, on_hour + step / 2.0));
        let (_, minute) = angles_for_time(0, 30, 0);
        assert!(approx(minute, core::f32::consts::PI));
    }

    #[test]
    fn per_hand_spec_gains_clamp_like_the_global_one() {
        let mut scene = ClockScene::for_size(600, 600);
        scene.hour_spec = 9.0;
        scene.minute_spec = -2.0;
        let c = scene.clamped();
        assert_eq!(c.hour_spec, 4.0);
        assert_eq!(c.minute_spec, 0.0);
        scene.hour_spec = f32::NAN;
        assert!(approx(scene.clamped().hour_spec, 1.0));
    }

    #[test]
    fn hand_darkness_defaults_to_zero_and_clamps() {
        assert_eq!(ClockScene::for_size(600, 600).hand_darkness, 0.0);
        let mut scene = ClockScene::for_size(600, 600);
        scene.hand_darkness = 2.0;
        assert_eq!(scene.clamped().hand_darkness, 1.0);
        scene.hand_darkness = -1.0;
        assert_eq!(scene.clamped().hand_darkness, 0.0);
        scene.hand_darkness = f32::NAN;
        assert_eq!(scene.clamped().hand_darkness, 0.0);
    }

    #[test]
    fn clamp_holds_every_range() {
        let mut scene = ClockScene::for_size(600, 600);
        scene.samples = 200;
        scene.shininess = -3.0;
        scene.ambient = 9.0;
        scene.light_h = 0.0;
        scene.hour_h = -1.0;
        scene.light_x = f32::NAN;
        let c = scene.clamped();
        assert_eq!(c.samples, MAX_SAMPLES);
        assert_eq!(c.shininess, 1.0);
        assert_eq!(c.ambient, 1.0);
        assert_eq!(c.light_h, 0.05);
        assert_eq!(c.hour_h, 0.0);
        assert!(approx(c.light_x, 0.45));
    }
}
