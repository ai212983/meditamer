//! Regional dithering metadata: per-pixel regions and per-region algorithms.
//!
//! The picture is still composed exactly once — [`render_region_row`]
//! shades the same pixels [`crate::render_gray_row`] shades and records,
//! alongside each gray byte, which semantic region that pixel belongs to:
//!
//! - [`DitherRegion::Background`]: bare dial paper.
//! - [`DitherRegion::Clock`]: dial furniture (ticks, hub).
//! - [`DitherRegion::Hands`]: visible hand sprites (composited coverage).
//! - [`DitherRegion::Shadows`]: overrides the base above wherever the
//!   *visible receiver's* shadow runs deep. Strength is `1 - visibility`
//!   at the receiver the viewer actually sees — the top hand when it
//!   covers, else the lower hand, else the dial — so a hidden lower hand's
//!   shadow can never claim the pixel. Selection is a smooth stochastic
//!   rounding: a fixed screen-anchored integer hash (independent of every
//!   threshold pattern, so selection never moirés with dither) is compared
//!   against the strength, giving penumbrae a speckled, weighted edge
//!   rather than an arbitrary rectangle. Style/profile choices never feed
//!   back into the gray plane.
//!
//! Dithering itself happens later, once, in [`crate::dither_regions`]:
//! ordered methods ([`DitherAlgorithm::Threshold`], [`DitherAlgorithm::Gradient`],
//! [`DitherAlgorithm::Bayer4`], [`DitherAlgorithm::Bayer8`],
//! [`DitherAlgorithm::BlueNoise`]) are pure threshold fields, while
//! [`DitherAlgorithm::FloydSteinberg`] and [`DitherAlgorithm::Atkinson`]
//! diffuse quantization error serpentine over the final composed gray.

use super::assets::Hands;
use super::render::{gray_byte, pixel_composite_prepared, Prepared};
use super::scene::ClockScene;
use blue_noise::BlueNoiseMap;

/// Semantic region of one composed pixel. The labels steer dithering only;
/// they never change the grayscale underneath.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DitherRegion {
    Background = 0,
    Clock = 1,
    Hands = 2,
    Shadows = 3,
}

/// Dither algorithm selectable per region. The first five are threshold
/// fields (cheap, deterministic, position-only); the last two are
/// serpentine error-diffusion passes over the composed gray.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DitherAlgorithm {
    Threshold,
    Gradient,
    Bayer4,
    Bayer8,
    BlueNoise,
    FloydSteinberg,
    Atkinson,
}

/// One algorithm per region. Defaults to [`DitherAlgorithm::Gradient`]
/// everywhere, matching the legacy single-dither look.
#[derive(Clone, Copy, Debug)]
pub struct RegionDithers {
    pub background: DitherAlgorithm,
    pub clock: DitherAlgorithm,
    pub hands: DitherAlgorithm,
    pub shadows: DitherAlgorithm,
}

impl Default for RegionDithers {
    fn default() -> Self {
        Self {
            background: DitherAlgorithm::Gradient,
            clock: DitherAlgorithm::Gradient,
            hands: DitherAlgorithm::Gradient,
            shadows: DitherAlgorithm::Gradient,
        }
    }
}

impl RegionDithers {
    /// The algorithm in force for one region label.
    pub(crate) fn for_region(self, region: DitherRegion) -> DitherAlgorithm {
        match region {
            DitherRegion::Background => self.background,
            DitherRegion::Clock => self.clock,
            DitherRegion::Hands => self.hands,
            DitherRegion::Shadows => self.shadows,
        }
    }

    /// Whether every region selects the same algorithm. The diffusion path
    /// uses this to run canonical whole-image diffusion instead of gating
    /// error on semantic labels that change nothing.
    pub(crate) fn is_uniform(self) -> bool {
        self.background == self.clock
            && self.background == self.hands
            && self.background == self.shadows
    }
}

/// What can go wrong before a single pixel is dithered. All variants are
/// caller-fixable geometry or buffer sizes; the gray and region planes are
/// never partially consumed on error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionalDitherError {
    /// `width` or `height` is zero: there is no frame to dither.
    EmptyFrame,
    /// `width * height` (or the scratch shape) overflows `usize`.
    FrameTooLarge,
    /// The gray plane is short for `width * height`.
    GrayTooShort { need: usize, got: usize },
    /// The region plane is short for `width * height`.
    RegionsTooShort { need: usize, got: usize },
    /// The caller scratch is short for
    /// [`crate::regional_scratch_len`]`(width)`.
    ScratchTooShort { need: usize, got: usize },
    /// The surface is smaller than `width` x `height`.
    SurfaceTooSmall,
}

impl core::fmt::Display for RegionalDitherError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            RegionalDitherError::EmptyFrame => write!(f, "zero-size frame"),
            RegionalDitherError::FrameTooLarge => write!(f, "frame overflows usize"),
            RegionalDitherError::GrayTooShort { need, got } => {
                write!(f, "gray plane holds {got}, needs {need}")
            }
            RegionalDitherError::RegionsTooShort { need, got } => {
                write!(f, "region plane holds {got}, needs {need}")
            }
            RegionalDitherError::ScratchTooShort { need, got } => {
                write!(f, "error scratch holds {got}, needs {need}")
            }
            RegionalDitherError::SurfaceTooSmall => write!(f, "surface smaller than frame"),
        }
    }
}

/// Interleaved gradient noise (Jimenez): the same three-multiply field the
/// legacy [`raster::Dither::Gradient`] resolves, kept identical so the
/// default profile matches the established tone.
fn gradient_threshold(x: i32, y: i32) -> f32 {
    let magic = 0.06711056 * x as f32 + 0.00583715 * y as f32;
    let fract = magic - libm::floorf(magic);
    let scaled = 52.982_918 * fract;
    scaled - libm::floorf(scaled)
}

fn bayer4_threshold(x: i32, y: i32) -> f32 {
    const MATRIX: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let cell = MATRIX[y.rem_euclid(4) as usize][x.rem_euclid(4) as usize];
    (f32::from(cell) + 0.5) * (1.0 / 16.0)
}

fn bayer8_threshold(x: i32, y: i32) -> f32 {
    #[rustfmt::skip]
    const MATRIX: [[u8; 8]; 8] = [
        [ 0, 32,  8, 40,  2, 34, 10, 42],
        [48, 16, 56, 24, 50, 18, 58, 26],
        [12, 44,  4, 36, 14, 46,  6, 38],
        [60, 28, 52, 20, 62, 30, 54, 22],
        [ 3, 35, 11, 43,  1, 33,  9, 41],
        [51, 19, 59, 27, 49, 17, 57, 25],
        [15, 47,  7, 39, 13, 45,  5, 37],
        [63, 31, 55, 23, 61, 29, 53, 21],
    ];
    let cell = MATRIX[y.rem_euclid(8) as usize][x.rem_euclid(8) as usize];
    (f32::from(cell) + 0.5) * (1.0 / 64.0)
}

/// Pick the full-screen blue-noise map for one frame, once per frame: the
/// map whose dimensions match `width` x `height` exactly, else the largest
/// enabled map. A smaller experimental frame reads the top-left window of
/// the larger map; a larger experimental frame wraps toroidally over it —
/// the field is never rescaled and no 32-cell tile repeats anywhere.
/// Returns `None` only when no map feature is enabled at all (this
/// prototype always enables both, so that is unreachable in practice).
pub(crate) fn blue_map_for_frame(width: u32, height: u32) -> Option<BlueNoiseMap> {
    blue_noise::map_for_size(width, height).or_else(blue_noise::default_map)
}

/// Threshold in `0.0..1.0` for an ordered algorithm at a screen pixel, or
/// `None` for the diffusion algorithms (those quantize at fixed mid-gray in
/// the error path and never query a field). `blue` is the frame's map from
/// [`blue_map_for_frame`], selected once per frame by the caller — never
/// per pixel. A [`DitherAlgorithm::BlueNoise`] pixel with no map enabled
/// also yields `None` and takes the diffusion-path mid-gray decision rather
/// than inventing a threshold.
pub(crate) fn ordered_threshold(
    algo: DitherAlgorithm,
    x: i32,
    y: i32,
    blue: Option<BlueNoiseMap>,
) -> Option<f32> {
    match algo {
        DitherAlgorithm::Threshold => Some(0.5),
        DitherAlgorithm::Gradient => Some(gradient_threshold(x, y)),
        DitherAlgorithm::Bayer4 => Some(bayer4_threshold(x, y)),
        DitherAlgorithm::Bayer8 => Some(bayer8_threshold(x, y)),
        DitherAlgorithm::BlueNoise => blue.map(|m| m.threshold(x, y)),
        DitherAlgorithm::FloydSteinberg | DitherAlgorithm::Atkinson => None,
    }
}

/// Fixed screen-anchored selector hash in `0.0..1.0`: a two-round
/// multiply-xorshift over the pixel coordinate with a fixed key. Integer
/// arithmetic, deliberately unrelated to the gradient/Bayer/blue phases, so
/// shadow selection cannot beat against any threshold pattern. No time seed:
/// the same pixel always decides the same way.
pub(crate) fn selector_noise(x: u32, y: u32) -> f32 {
    let mut h = x
        .wrapping_mul(0x85EB_CA6B)
        .wrapping_add(y.wrapping_mul(0xC2B2_AE35))
        .wrapping_add(0x27D4_EB2D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    // Exact 24-bit fraction: every output is a distinct multiple of 2^-24.
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Composited hand coverage that reads as "hand": either layer alone past
/// half coverage, or both layers together past half.
const HAND_COVER: f32 = 0.5;

/// Composited coverage of one pixel from its two painted-layer alphas,
/// clamped to the unit range. Shared by the legacy label below and the
/// composition experiment's physical footprint, so the two can never drift.
pub(crate) fn composite_cover(lower_alpha: f32, upper_alpha: f32) -> f32 {
    1.0 - (1.0 - lower_alpha.clamp(0.0, 1.0)) * (1.0 - upper_alpha.clamp(0.0, 1.0))
}

/// The legacy semantic label for one composite: hands past half coverage,
/// else dial furniture, else paper, with the stochastic shadow override from
/// the visible receiver's actual shading. Style/profile choices never feed
/// back into the gray plane. Shared with the composition path so its full
/// labels stay byte-identical to this function.
pub(crate) fn classify_region(
    cover: f32,
    dial_clock: bool,
    receiver_vis: f32,
    x: u32,
    y: u32,
) -> DitherRegion {
    let mut region = if cover >= HAND_COVER {
        DitherRegion::Hands
    } else if dial_clock {
        DitherRegion::Clock
    } else {
        DitherRegion::Background
    };
    // Shadow override from the visible receiver's actual shading: only
    // pixels the light genuinely struggles to reach flip, with weight
    // proportional to the shortfall.
    let strength = 1.0 - receiver_vis;
    if strength > 0.0 && selector_noise(x, y) < strength {
        region = DitherRegion::Shadows;
    }
    region
}

/// One grayscale row plus its region labels. The gray bytes are identical to
/// [`crate::render_gray_row`] for the same scene, hands, and row: regions
/// are metadata, never a second rendering. Writes `min(gray.len(),
/// regions.len(), width)` pixels; a `y` past the scene height is a no-op.
/// Constant scratch only.
pub fn render_region_row(
    scene: &ClockScene,
    hands: &Hands<'_>,
    y: u32,
    gray: &mut [u8],
    regions: &mut [DitherRegion],
) {
    let scene = scene.clamped();
    if y >= scene.height {
        return;
    }
    let ok = hands.validate();
    let prep = Prepared::for_frame(&scene, hands);
    let n = gray.len().min(regions.len()).min(scene.width as usize);
    for x in 0..n {
        let c = pixel_composite_prepared(&scene, hands, ok, &prep, x as u32, y);
        gray[x] = gray_byte(c.rgb);
        let cover = composite_cover(c.lower_alpha, c.upper_alpha);
        regions[x] = classify_region(cover, c.dial_clock, c.receiver_vis, x as u32, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raster::Dither;

    #[test]
    fn shared_thresholds_match_the_raster_contract() {
        // The regional path must resolve the same tone as the legacy
        // single-dither surface for the three algorithms both know.
        let pairs = [
            (DitherAlgorithm::Threshold, Dither::None),
            (DitherAlgorithm::Gradient, Dither::Gradient),
            (DitherAlgorithm::Bayer4, Dither::Bayer4),
        ];
        for (algo, legacy) in pairs {
            for y in 0..16 {
                for x in 0..16 {
                    let got = ordered_threshold(algo, x, y, None).expect("ordered");
                    assert!(
                        (got - legacy.threshold(x, y)).abs() < 1e-6,
                        "{algo:?} diverged from raster at ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn blue_noise_is_not_renamed_gradient() {
        let blue_map = blue_map_for_frame(600, 600);
        assert!(blue_map.is_some(), "prototype enables both maps");
        let mut same = 0;
        for y in 0..64 {
            for x in 0..64 {
                let blue =
                    ordered_threshold(DitherAlgorithm::BlueNoise, x, y, blue_map).expect("ordered");
                if (blue - gradient_threshold(x, y)).abs() < 1e-6 {
                    same += 1;
                }
            }
        }
        assert!(same < 2048, "suspicious overlap with gradient: {same}/4096");
    }

    #[test]
    fn selector_is_deterministic_bounded_and_spatial() {
        let a = selector_noise(7, 41);
        assert_eq!(a, selector_noise(7, 41));
        assert!((0.0..1.0).contains(&a));
        let mut distinct = 0;
        for x in 0..32 {
            if selector_noise(x, 5) != a {
                distinct += 1;
            }
        }
        assert!(distinct > 24, "selector barely varies across the row");
    }

    #[test]
    fn diffusion_algorithms_have_no_threshold_field() {
        assert_eq!(
            ordered_threshold(DitherAlgorithm::FloydSteinberg, 0, 0, None),
            None
        );
        assert_eq!(
            ordered_threshold(DitherAlgorithm::Atkinson, 3, 9, None),
            None
        );
    }
}
