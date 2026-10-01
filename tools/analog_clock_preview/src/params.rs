//! One parameter set for every entry point: CLI flags and HTTP query
//! pairs land on the same struct with the same bounds, then build one
//! [`analog_clock::ClockScene`]. Out-of-range or unparseable input is an
//! error string, never a panic or a silent clamp.

use std::sync::Arc;

use analog_clock::{
    angles_for_time, ClockScene, CompositionMode, DitherAlgorithm, DitherRegion, RegionDithers,
};

use crate::assets::DecodedDial;

/// Grayscale core output, or the core output dithered once per region for
/// the panel. Dithered is the default so the four pattern selectors below
/// have a visible effect; gray stays one click away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputMode {
    Gray,
    #[default]
    Dithered,
    Changes,
}

/// Everything the preview can vary, already validated.
#[derive(Clone, Debug)]
pub struct PreviewParams {
    pub width: u32,
    pub height: u32,
    pub hours: u8,
    pub minutes: u8,
    pub dial_fraction: f32,
    pub light_x: f32,
    pub light_y: f32,
    pub light_h: f32,
    pub light_size: f32,
    pub hour_h: f32,
    pub minute_h: f32,
    pub spec_strength: f32,
    pub hour_spec: f32,
    pub minute_spec: f32,
    pub shininess: f32,
    /// Hand shade attenuation 0..1 (default 0.85: readable dark hands).
    /// Scales each hand's linear-light shade before compositing; the
    /// dither algorithm choice stays independent.
    pub hand_darkness: f32,
    pub samples: u8,
    pub mode: OutputMode,
    pub composition: CompositionMode,
    pub compare_hours: u8,
    pub compare_minutes: u8,
    pub diagnostics: bool,
    /// One dot pattern per region; every region accepts any of the seven
    /// methods. Selections never change the lighting, only the final
    /// binary quantization of the already-composed grayscale.
    pub regions: RegionDithers,
    pub hour_pivot: Option<(f32, f32)>,
    pub minute_pivot: Option<(f32, f32)>,
    /// Supplied dial artwork, shared by `Arc`: `None` keeps the procedural
    /// dial, `Some` borrows its pixels into every scene this builds.
    /// Cloning params stays cheap; no host asset reference reaches firmware.
    pub dial: Option<Arc<DecodedDial>>,
}

impl Default for PreviewParams {
    fn default() -> Self {
        Self {
            width: 600,
            height: 600,
            hours: 10,
            minutes: 9,
            dial_fraction: 0.96,
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
            hand_darkness: 0.85,
            samples: 8,
            mode: OutputMode::Dithered,
            composition: CompositionMode::Legacy,
            compare_hours: 10,
            compare_minutes: 9,
            diagnostics: false,
            // All Gradient preserves the pre-regional baseline appearance.
            regions: RegionDithers::default(),
            hour_pivot: None,
            minute_pivot: None,
            dial: None,
        }
    }
}

fn num<T: std::str::FromStr>(raw: &str, name: &str) -> Result<T, String> {
    raw.parse()
        .map_err(|_| format!("{name} must be a number, got {raw:?}"))
}

/// Parses one dot-pattern method for any region. `none` is the historic
/// name for the plain 50% cut-off (Threshold).
fn parse_algorithm(value: &str) -> Result<DitherAlgorithm, String> {
    match value {
        "none" => Ok(DitherAlgorithm::Threshold),
        "gradient" => Ok(DitherAlgorithm::Gradient),
        "bayer4" => Ok(DitherAlgorithm::Bayer4),
        "bayer8" => Ok(DitherAlgorithm::Bayer8),
        "blue-noise" => Ok(DitherAlgorithm::BlueNoise),
        "floyd-steinberg" => Ok(DitherAlgorithm::FloydSteinberg),
        "atkinson" => Ok(DitherAlgorithm::Atkinson),
        _ => Err(
            "dither pattern must be one of none, gradient, bayer4, bayer8, blue-noise, floyd-steinberg, atkinson"
                .to_string(),
        ),
    }
}

fn ranged(value: f32, lo: f32, hi: f32, name: &str) -> Result<f32, String> {
    if !value.is_finite() || value < lo || value > hi {
        return Err(format!("{name} must be {lo}..={hi}, got {value}"));
    }
    Ok(value)
}

impl PreviewParams {
    /// Applies one `key=value` pair; unknown keys are an error so typos
    /// surface instead of silently rendering defaults.
    pub fn apply(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "w" | "h" | "time" => self.apply_geometry(key, value),
            "dial" | "light_x" | "light_y" | "light_h" | "light_size" => {
                self.apply_lighting(key, value)
            }
            "hour_h" | "minute_h" | "spec" | "hour_spec" | "minute_spec" | "shiny"
            | "hand_darkness" => self.apply_material(key, value),
            "samples" | "mode" | "composition" | "compare_time" | "diagnostics" => {
                self.apply_display(key, value)
            }
            "dither" | "background_dither" | "clock_dither" | "hands_dither" | "shadows_dither" => {
                self.apply_dither(key, value)
            }
            "hour_px" | "hour_py" | "minute_px" | "minute_py" => self.apply_pivot(key, value),
            _ => Err(format!("unknown setting {key:?}")),
        }
    }

    fn apply_geometry(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "w" => {
                let v: u32 = num(value, "w")?;
                if !(64..=1200).contains(&v) {
                    return Err("w must be 64..=1200".to_string());
                }
                self.width = v;
            }
            "h" => {
                let v: u32 = num(value, "h")?;
                if !(64..=1200).contains(&v) {
                    return Err("h must be 64..=1200".to_string());
                }
                self.height = v;
            }
            "time" => {
                let (h, m) = value
                    .split_once(':')
                    .ok_or_else(|| "time must look like 10:09".to_string())?;
                let h: u8 = num(h, "hours")?;
                let m: u8 = num(m, "minutes")?;
                if h > 23 || m > 59 {
                    return Err(format!("time must be 00:00..=23:59, got {value:?}"));
                }
                self.hours = h;
                self.minutes = m;
            }
            _ => unreachable!("dispatched key group"),
        }
        Ok(())
    }

    fn apply_lighting(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "dial" => self.dial_fraction = ranged(num(value, "dial")?, 0.5, 1.0, "dial")?,
            "light_x" => self.light_x = ranged(num(value, "light_x")?, -2.0, 2.0, "light_x")?,
            "light_y" => self.light_y = ranged(num(value, "light_y")?, -2.0, 2.0, "light_y")?,
            "light_h" => self.light_h = ranged(num(value, "light_h")?, 0.05, 4.0, "light_h")?,
            "light_size" => {
                self.light_size = ranged(num(value, "light_size")?, 0.0, 0.6, "light_size")?
            }
            _ => unreachable!("dispatched key group"),
        }
        Ok(())
    }

    fn apply_material(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "hour_h" => self.hour_h = ranged(num(value, "hour_h")?, 0.0, 0.5, "hour_h")?,
            "minute_h" => self.minute_h = ranged(num(value, "minute_h")?, 0.0, 0.5, "minute_h")?,
            "spec" => self.spec_strength = ranged(num(value, "spec")?, 0.0, 4.0, "spec")?,
            "hour_spec" => {
                self.hour_spec = ranged(num(value, "hour_spec")?, 0.0, 4.0, "hour_spec")?;
            }
            "minute_spec" => {
                self.minute_spec = ranged(num(value, "minute_spec")?, 0.0, 4.0, "minute_spec")?;
            }
            "shiny" => self.shininess = ranged(num(value, "shiny")?, 1.0, 256.0, "shiny")?,
            "hand_darkness" => {
                self.hand_darkness =
                    ranged(num(value, "hand_darkness")?, 0.0, 1.0, "hand_darkness")?
            }
            _ => unreachable!("dispatched key group"),
        }
        Ok(())
    }

    fn apply_display(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "samples" => {
                let v: u8 = num(value, "samples")?;
                if !(1..=24).contains(&v) {
                    return Err("samples must be 1..=24".to_string());
                }
                self.samples = v;
            }
            "mode" => {
                self.mode = match value {
                    "gray" => OutputMode::Gray,
                    "dither" => OutputMode::Dithered,
                    "changes" => OutputMode::Changes,
                    _ => return Err("mode must be gray, dither, or changes".to_string()),
                }
            }
            "composition" => {
                self.composition = CompositionMode::from_str(value).ok_or_else(|| {
                    "composition must be one of legacy, reference, replacement, removal".to_string()
                })?;
            }
            "compare_time" => {
                let (h, m) = value
                    .split_once(':')
                    .ok_or_else(|| "compare_time must look like 10:09".to_string())?;
                let h: u8 = num(h, "compare_hours")?;
                let m: u8 = num(m, "compare_minutes")?;
                if h > 23 || m > 59 {
                    return Err(format!("compare_time must be 00:00..=23:59, got {value:?}"));
                }
                self.compare_hours = h;
                self.compare_minutes = m;
            }
            "diagnostics" => {
                self.diagnostics = match value {
                    "0" => false,
                    "1" => true,
                    _ => return Err("diagnostics must be 0 or 1".to_string()),
                };
            }
            _ => unreachable!("dispatched key group"),
        }
        Ok(())
    }

    fn apply_dither(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            // Legacy global key: sets all four regional profiles at once,
            // so old `--dither` / `?dither=` callers keep working.
            "dither" => {
                let method = parse_algorithm(value)?;
                self.regions = RegionDithers {
                    background: method,
                    clock: method,
                    hands: method,
                    shadows: method,
                };
            }
            "background_dither" => self.regions.background = parse_algorithm(value)?,
            "clock_dither" => self.regions.clock = parse_algorithm(value)?,
            "hands_dither" => self.regions.hands = parse_algorithm(value)?,
            "shadows_dither" => self.regions.shadows = parse_algorithm(value)?,
            _ => unreachable!("dispatched key group"),
        }
        Ok(())
    }

    fn apply_pivot(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            // Same bounds as the core hand map validation.
            "hour_px" => {
                let v: f32 = num(value, "hour_px")?;
                if !v.is_finite() || !(0.0..245.0).contains(&v) {
                    return Err(format!("hour_px must be 0..245, got {v}"));
                }
                self.hour_pivot = Some((v, self.hour_pivot.unwrap_or(analog_clock::HOUR_PIVOT).1));
            }
            "hour_py" => {
                let v: f32 = num(value, "hour_py")?;
                if !v.is_finite() || v <= 0.0 || v >= 810.0 {
                    return Err(format!("hour_py must be 0..810 with y > 0, got {v}"));
                }
                self.hour_pivot = Some((self.hour_pivot.unwrap_or(analog_clock::HOUR_PIVOT).0, v));
            }
            "minute_px" => {
                let v: f32 = num(value, "minute_px")?;
                if !v.is_finite() || !(0.0..156.0).contains(&v) {
                    return Err(format!("minute_px must be 0..156, got {v}"));
                }
                self.minute_pivot =
                    Some((v, self.minute_pivot.unwrap_or(analog_clock::MINUTE_PIVOT).1));
            }
            "minute_py" => {
                let v: f32 = num(value, "minute_py")?;
                if !v.is_finite() || v <= 0.0 || v >= 1014.0 {
                    return Err(format!("minute_py must be 0..1014 with y > 0, got {v}"));
                }
                self.minute_pivot =
                    Some((self.minute_pivot.unwrap_or(analog_clock::MINUTE_PIVOT).0, v));
            }
            _ => unreachable!("dispatched key group"),
        }
        Ok(())
    }

    /// Builds the shared-core scene. Radius derives from the output size
    /// and the dial fraction, so layout stays proportional. Pivots are NOT
    /// part of the scene: [`hands`] applies the overrides to the borrowed
    /// maps, the single authority for both scale and offset. The supplied
    /// dial (when set) is borrowed into the scene automatically, so every
    /// existing `render_*` caller renders the same image with no extra
    /// argument.
    pub fn scene(&self) -> ClockScene<'_> {
        let (hour_angle, minute_angle) = angles_for_time(self.hours, self.minutes, 0);
        let short = std::cmp::min(self.width, self.height) as f32;
        ClockScene {
            dial: self.dial.as_deref().map(DecodedDial::as_dial),
            width: self.width,
            height: self.height,
            center_x: self.width as f32 / 2.0,
            center_y: self.height as f32 / 2.0,
            radius: 0.5 * self.dial_fraction * short,
            hour_angle,
            minute_angle,
            hour_len: 0.55,
            minute_len: 0.85,
            light_x: self.light_x,
            light_y: self.light_y,
            light_h: self.light_h,
            light_size: self.light_size,
            hour_h: self.hour_h,
            minute_h: self.minute_h,
            spec_strength: self.spec_strength,
            hour_spec: self.hour_spec,
            minute_spec: self.minute_spec,
            shininess: self.shininess,
            hand_darkness: self.hand_darkness,
            ambient: 0.25,
            samples: self.samples,
        }
        .clamped()
    }
}

/// Borrows decoded planes with this run's pivot overrides applied.
/// Overrides are the single authority for scale and offset: when set they
/// replace the borrowed pivots on both the gray and dither paths (both go
/// through here); when unset the borrowed defaults stand.
pub fn hands<'a>(
    params: &PreviewParams,
    decoded: &'a crate::assets::DecodedHands,
) -> analog_clock::Hands<'a> {
    let mut borrowed = decoded.as_hands();
    if let Some((hx, hy)) = params.hour_pivot {
        borrowed.hour.pivot_x = hx;
        borrowed.hour.pivot_y = hy;
    }
    if let Some((mx, my)) = params.minute_pivot {
        borrowed.minute.pivot_x = mx;
        borrowed.minute.pivot_y = my;
    }
    borrowed
}

/// Renders grayscale bytes (row-major, one per pixel) through the shared
/// core: the single path CLI exports and the server encodes.
pub fn render_gray(params: &PreviewParams, hands: &analog_clock::Hands<'_>) -> Vec<u8> {
    let scene = params.scene();
    let mut out = vec![0u8; params.width as usize * params.height as usize];
    let w = params.width as usize;
    for (y, row) in out.chunks_exact_mut(w).enumerate() {
        analog_clock::render_gray_row(&scene, hands, y as u32, row);
    }
    out
}

/// Composes the full frame through the shared core: grayscale lighting
/// plus the per-pixel region plane, one [`analog_clock::render_region_row`]
/// call per row. Dimensions come from validated params (64..=1200), so the
/// host heap planes are bounded; this full-frame prototype path is a host
/// convenience, not a device-memory-fit claim.
pub fn render_planes(
    params: &PreviewParams,
    hands: &analog_clock::Hands<'_>,
) -> (Vec<u8>, Vec<DitherRegion>) {
    let scene = params.scene();
    let (w, h) = (params.width as usize, params.height as usize);
    let mut gray = vec![0u8; w * h];
    let mut regions = vec![DitherRegion::Background; w * h];
    for y in 0..params.height {
        let row = y as usize * w;
        analog_clock::render_region_row(
            &scene,
            hands,
            y,
            &mut gray[row..row + w],
            &mut regions[row..row + w],
        );
    }
    (gray, regions)
}

/// Renders a 1-bit dithered canvas through the shared regional core: the
/// composed grayscale quantized exactly once per pixel with that pixel's
/// region profile. The host owns every buffer (gray plane, region plane,
/// caller `f32` scratch); the core allocates nothing.
pub fn render_dithered(params: &PreviewParams, hands: &analog_clock::Hands<'_>) -> Vec<u8> {
    let (gray, regions) = render_planes(params, hands);
    let w = params.width as i32;
    let h = params.height as i32;
    let scratch = analog_clock::regional_scratch_len(params.width as usize)
        .expect("validated width overflows regional scratch");
    let mut errors = vec![0f32; scratch];
    let mut bits = vec![0u8; raster::BitCanvas::bytes_for(w, h)];
    {
        let mut canvas = raster::BitCanvas::new(w, h, &mut bits).expect("canvas");
        analog_clock::dither_regions(
            params.width,
            params.height,
            &gray,
            &regions,
            &params.regions,
            &mut errors,
            &mut canvas,
        )
        .expect("composed planes match validated geometry");
    }
    bits
}

/// Expands packed MSB-first ink bits (`1` = ink) back to grayscale bytes
/// for PNG transport. Shared by the legacy path and the composed frames.
pub fn bits_to_gray(bits: &[u8], pixels: usize) -> Vec<u8> {
    let mut out = vec![0u8; pixels];
    for (i, slot) in out.iter_mut().enumerate() {
        let ink = bits[i / 8] & (0x80 >> (i % 8)) != 0;
        *slot = if ink { 0 } else { 255 };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const METHODS: [(&str, DitherAlgorithm); 7] = [
        ("none", DitherAlgorithm::Threshold),
        ("gradient", DitherAlgorithm::Gradient),
        ("bayer4", DitherAlgorithm::Bayer4),
        ("bayer8", DitherAlgorithm::Bayer8),
        ("blue-noise", DitherAlgorithm::BlueNoise),
        ("floyd-steinberg", DitherAlgorithm::FloydSteinberg),
        ("atkinson", DitherAlgorithm::Atkinson),
    ];

    fn profile(params: &PreviewParams) -> [DitherAlgorithm; 4] {
        [
            params.regions.background,
            params.regions.clock,
            params.regions.hands,
            params.regions.shadows,
        ]
    }

    #[test]
    fn defaults_are_dithered_all_gradient() {
        let params = PreviewParams::default();
        assert_eq!(params.mode, OutputMode::Dithered);
        assert!(
            profile(&params)
                .iter()
                .all(|m| *m == DitherAlgorithm::Gradient),
            "baseline appearance must stay all-Gradient"
        );
    }

    #[test]
    fn every_method_parses_for_every_region() {
        for key in [
            "background_dither",
            "clock_dither",
            "hands_dither",
            "shadows_dither",
        ] {
            for (raw, method) in METHODS {
                let mut params = PreviewParams::default();
                params.apply(key, raw).expect("valid method");
                let got = match key {
                    "background_dither" => params.regions.background,
                    "clock_dither" => params.regions.clock,
                    "hands_dither" => params.regions.hands,
                    _ => params.regions.shadows,
                };
                assert_eq!(got, method, "{key}={raw}");
                // A single-region key leaves the other three on Gradient.
                let untouched: Vec<DitherAlgorithm> = profile(&params)
                    .into_iter()
                    .zip(["background", "clock", "hands", "shadows"])
                    .filter(|(_, name)| format!("{name}_dither") != key)
                    .map(|(m, _)| m)
                    .collect();
                assert!(
                    untouched.iter().all(|m| *m == DitherAlgorithm::Gradient),
                    "{key}={raw} leaked into {untouched:?}"
                );
            }
        }
    }

    #[test]
    fn legacy_global_key_sets_all_four() {
        // Old values keep working; new values ride the same key.
        for (raw, method) in METHODS {
            let mut params = PreviewParams::default();
            params.apply("dither", raw).expect("valid method");
            assert!(
                profile(&params).iter().all(|m| *m == method),
                "dither={raw} did not set all four"
            );
        }
    }

    #[test]
    fn hand_darkness_default_bounds_and_scene() {
        assert!((PreviewParams::default().hand_darkness - 0.85).abs() < 1e-6);
        for good in ["0", "1", "0.5", "0.85"] {
            let mut params = PreviewParams::default();
            params.apply("hand_darkness", good).expect("valid darkness");
        }
        for bad in ["nan", "NaN", "inf", "-0.1", "1.5", "abc", ""] {
            let mut params = PreviewParams::default();
            assert!(
                params.apply("hand_darkness", bad).is_err(),
                "hand_darkness={bad} should fail"
            );
        }
        let mut params = PreviewParams::default();
        params.apply("hand_darkness", "0.5").unwrap();
        assert!((params.scene().hand_darkness - 0.5).abs() < 1e-6);
        assert!((PreviewParams::default().scene().hand_darkness - 0.85).abs() < 1e-6);
    }

    #[test]
    fn bad_pattern_values_are_errors() {
        for (key, value) in [
            ("dither", "bayer"),
            ("dither", ""),
            ("dither", "Floyd-Steinberg"),
            ("background_dither", "none "),
            ("background_dither", "ordered"),
            ("clock_dither", "bayer16"),
            ("hands_dither", "atkinson2"),
            ("shadows_dither", "diffusion"),
        ] {
            let mut params = PreviewParams::default();
            assert!(
                params.apply(key, value).is_err(),
                "{key}={value} should fail"
            );
        }
    }
}
