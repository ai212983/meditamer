//! The synthetic ink field: per-bristle coverage against position along the
//! stroke, generated rather than sampled from a photograph.
//!
//! An earlier version of this model read a real brush stroke's texture off a
//! reference photograph. That carried more structure than this does -- rank
//! eight against rank two -- but nearly all of the surplus was high frequency
//! *across* the bristles, and thirty-two rows spanning a fifty-pixel mark sit
//! at the Nyquist limit for that: it cannot be resolved, so it aliases instead,
//! which is the moire that took several rounds to chase down. Generating the
//! field lets every part of it be band-limited by construction.
//!
//! Variation comes from four places, none of which needs high frequencies:
//! capacity varying smoothly along the bundle, hairs clumping so neighbours
//! behave alike, each hair drying at its own rate, and a few giving out
//! abruptly, which is what flying white is.
//!
//! Rows and columns here are a fixed grid -- [`ROWS`] positions across the
//! brush by [`S_SAMPLES`] positions along the stroke -- baked once per stroke
//! and then only ever bilinearly sampled. [`ROWS`] is thirty-two rather than
//! sixty-four for the same aliasing reason: see the module doc above.

use libm::{expf, sinf};

use crate::rng::Rng;

pub const S_SAMPLES: usize = 1200;
pub const ROWS: usize = 32;

const TAU: f32 = core::f32::consts::TAU;

/// Position along the stroke for column `col`, in `0.0..=1.0`.
#[inline]
pub(crate) fn column_s(col: usize) -> f32 {
    col as f32 / (S_SAMPLES - 1) as f32
}

/// Quantises a coverage value to the field's on-disk resolution. One part in
/// 255 is well under what a one-bit dither can resolve, so nothing downstream
/// notices the loss, and it is what keeps the field's footprint near 40 KB
/// instead of four times that in `f32`.
#[inline]
fn quantize(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
fn dequantize(value: u8) -> f32 {
    value as f32 * (1.0 / 255.0)
}

/// One row's raw coverage curve, before the vertical (across-bundle) blur.
///
/// Every row here is independent of every other -- capacity, clumping, dry-out
/// rate and the abrupt give-out are all per-row draws -- so this can be
/// generated one row at a time rather than holding the whole `ROWS x
/// S_SAMPLES` field in memory while it is built. Only the blur afterwards
/// needs neighbours, and it needs at most one row on each side.
fn wet_row(cap: f32, rate: f32, give: f32, rng: &mut Rng) -> [f32; S_SAMPLES] {
    let mut row = [0.0f32; S_SAMPLES];
    for (col, value) in row.iter_mut().enumerate() {
        let s = column_s(col);
        *value = (cap - rate * s).clamp(0.0, 1.0);
    }
    let k = rng.range(1.2, 4.5);
    let a = rng.range(0.03, 0.11);
    let phase = rng.range(0.0, TAU);
    for (col, value) in row.iter_mut().enumerate() {
        *value += a * sinf(column_s(col) * TAU * k + phase);
    }
    // A hair that gives out stays out -- once it has run dry it does not wet
    // again later in the stroke, which is what makes flying white a one-way
    // erosion rather than a texture that could in principle heal.
    if give < 1.5 {
        for (col, value) in row.iter_mut().enumerate() {
            *value *= ((give - column_s(col)) / 0.06).clamp(0.0, 1.0);
        }
    }
    for value in &mut row {
        *value = value.clamp(0.0, 1.0);
    }
    row
}

/// Builds the ink field and its per-column mean, in that order because the
/// mean is measured off this *ungrained* field -- see [`super::EnsoParams`]'s
/// grain step for why grain is baked in afterwards instead of here.
pub(crate) fn build(rng: &mut Rng) -> ([[u8; S_SAMPLES]; ROWS], [u8; S_SAMPLES]) {
    // Capacity along the bundle: a few low harmonics, so neighbouring hairs
    // are correlated and no detail lands above what the mark can show.
    let mut cap = [0.96f32; ROWS];
    for _ in 0..3 {
        let k = rng.range(0.6, 2.6);
        let a = rng.range(0.05, 0.16);
        let phase = rng.range(0.0, TAU);
        for (row, value) in cap.iter_mut().enumerate() {
            *value += a * sinf(row as f32 / (ROWS - 1) as f32 * TAU * k + phase);
        }
    }
    for value in &mut cap {
        *value = value.clamp(0.45, 1.0);
    }

    // Clumps: short runs of hairs sharing a bias, so the bundle is not a comb.
    let mut clump = [0.0f32; ROWS];
    let clump_count = rng.range(3.0, 7.0) as i32;
    for _ in 0..clump_count {
        let centre = rng.range(0.0, ROWS as f32);
        let width = rng.range(1.5, 4.5);
        let height = rng.range(-0.16, 0.08);
        for (row, value) in clump.iter_mut().enumerate() {
            let d = (row as f32 - centre) / width;
            *value += height * expf(-(d * d));
        }
    }
    for (row, value) in cap.iter_mut().enumerate() {
        *value = (*value + clump[row]).clamp(0.35, 1.0);
    }

    // Each hair gives out at its own rate, and some abruptly. `spend` is the
    // stroke's overall dryness, so one enso can stay loaded the whole way
    // round and another break up early.
    let spend = rng.range(0.30, 0.85);
    let mut rate = [0.0f32; ROWS];
    for value in &mut rate {
        *value = rng.range(0.35, 1.35) * spend;
    }
    let mut give = [0.0f32; ROWS];
    for value in &mut give {
        *value = if rng.unit() < 0.28 {
            rng.range(0.40, 1.6)
        } else {
            9.9
        };
    }

    let mut ink = [[0u8; S_SAMPLES]; ROWS];
    let mut mean_accum = [0.0f32; S_SAMPLES];
    let mut prev = [0.0f32; S_SAMPLES];
    let mut curr = wet_row(cap[0], rate[0], give[0], rng);
    for row in 0..ROWS {
        let next = if row + 1 < ROWS {
            wet_row(cap[row + 1], rate[row + 1], give[row + 1], rng)
        } else {
            [0.0f32; S_SAMPLES]
        };
        // Band-limit across the bundle, as the sampling requires -- see the
        // module doc. Zero-padded at both ends, matching a same-length
        // convolution against an odd kernel.
        //
        // Disabling this was tried and measured: oscillation goes from 0.019 to
        // 0.070, and the per-frame reverse transitions the panel has to drive
        // rise from 37 to 64. The artifact it suppresses is neighbouring
        // bristles trading intensity against the pixel grid, which is invisible
        // at thumbnail size and obvious at full resolution.
        for col in 0..S_SAMPLES {
            let blurred = 0.25 * prev[col] + 0.5 * curr[col] + 0.25 * next[col];
            ink[row][col] = quantize(blurred);
            mean_accum[col] += blurred;
        }
        prev = curr;
        curr = next;
    }

    let mut mean = [0u8; S_SAMPLES];
    for (col, value) in mean.iter_mut().enumerate() {
        *value = quantize(mean_accum[col] / ROWS as f32);
    }
    (ink, mean)
}

/// Reads back one baked ink value as coverage in `0.0..=1.0`.
#[inline]
pub(crate) fn read(ink: &[[u8; S_SAMPLES]; ROWS], row: usize, col: usize) -> f32 {
    dequantize(ink[row][col])
}

/// The column pair and blend fraction for position `s` (`0.0..=1.0`) along the
/// stroke, shared by every table this module and `params` index by arc
/// position: the ink field, `press_pow` and `u_max`.
#[inline]
pub(crate) fn index_for(s: f32) -> (usize, usize, f32) {
    let clamped = s.clamp(0.0, 1.0);
    let idx = clamped * (S_SAMPLES - 1) as f32;
    let i0 = idx as usize;
    let i1 = (i0 + 1).min(S_SAMPLES - 1);
    let fi = idx - i0 as f32;
    (i0, i1, fi)
}

/// Multiplies grain into an already-built field, in place.
///
/// This is a separate pass rather than something folded into [`build`] because
/// the grain seed and strength are drawn later in the parameter sequence than
/// the field itself, and drawing them early just to bake grain in one pass
/// would reorder every draw after them for no benefit -- see
/// `EnsoParams::from_seed`.
///
/// Baking here, rather than sampling per rendered pixel, is what removes grain
/// from the render's per-sample cost entirely: grain is a function of arc
/// length and row, the same two axes this field is already indexed by, so
/// evaluating it once per grid point and folding it into the stored value costs
/// nothing later. The one thing this changes from a literal per-pixel port is
/// order: the reference multiplies grain in after blending toward the tip's
/// converged mean, so the mean itself is unaffected by grain, while this bakes
/// grain in first and takes the mean of the *grained* field. The two agree
/// away from the tip (where the blend is 100% raw ink) and differ by a
/// second-order term near it, which is where the mark is thin enough that
/// grain is the least visible thing happening to it.
pub(crate) fn apply_grain(
    ink: &mut [[u8; S_SAMPLES]; ROWS],
    grain_strength: f32,
    grain_seed: u32,
    arc_per_column: f32,
) {
    for (row, columns) in ink.iter_mut().enumerate() {
        for (col, value) in columns.iter_mut().enumerate() {
            let arc = column_s(col) * arc_per_column;
            let noise = grain(arc, row as f32, grain_seed);
            let grained = dequantize(*value) * (1.0 - grain_strength * noise);
            *value = quantize(grained);
        }
    }
}

/// A hash of one grid cell into `0.0..1.0`. Coordinates come in signed because
/// the entry cap's rounding sits at negative arc length, behind the stroke's
/// own start.
fn grain_hash(a: i32, b: i32, seed: u32) -> f32 {
    let mut v = (a as u32).wrapping_mul(0x9E37_79B1) ^ (b as u32).wrapping_mul(0x85EB_CA77) ^ seed;
    v = (v ^ (v >> 16)).wrapping_mul(0x7FEB_352D);
    v = (v ^ (v >> 15)).wrapping_mul(0x846C_A68B);
    v ^= v >> 16;
    (v & 0x00FF_FFFF) as f32 * (1.0 / 16_777_216.0)
}

/// Value noise along the stroke and across the brush, interpolated in *both*.
///
/// Indexing this by the integer bristle row would change the hash at every row
/// boundary, and with thirty-two rows across a fifty-pixel mark that is most
/// adjacent pixel pairs -- the grain would turn into hard-edged steps running
/// along the mark instead of texture. The across-brush cell is also several
/// bristles wide on purpose: at one row it is under a pixel, so the grain
/// would alternate every pixel, which was a quarter of all the flicker in the
/// mark on its own.
fn grain(arc: f32, row: f32, seed: u32) -> f32 {
    let ax = arc / 5.0;
    let xi = libm::floorf(ax);
    let fx = ax - xi;
    let ry = row / 6.0;
    let yi = libm::floorf(ry);
    let fy = ry - yi;
    let (xi, yi) = (xi as i32, yi as i32);
    let a = grain_hash(xi, yi, seed);
    let b = grain_hash(xi + 1, yi, seed);
    let c = grain_hash(xi, yi + 1, seed);
    let d = grain_hash(xi + 1, yi + 1, seed);
    let tx = fx * fx * (3.0 - 2.0 * fx);
    let ty = fy * fy * (3.0 - 2.0 * fy);
    (a * (1.0 - tx) + b * tx) * (1.0 - ty) + (c * (1.0 - tx) + d * tx) * ty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_reproduces_its_field() {
        let mut first = Rng::new(42);
        let mut second = Rng::new(42);
        let (ink_a, mean_a) = build(&mut first);
        let (ink_b, mean_b) = build(&mut second);
        assert_eq!(ink_a, ink_b);
        assert_eq!(mean_a, mean_b);
    }

    #[test]
    fn stays_inside_the_unit_range() {
        let mut rng = Rng::new(7);
        let (ink, mean) = build(&mut rng);
        for row in &ink {
            for &value in row {
                assert!(dequantize(value) <= 1.0);
            }
        }
        for &value in &mean {
            assert!(dequantize(value) <= 1.0);
        }
    }

    #[test]
    fn grain_stays_inside_the_unit_range() {
        for step in -200..200 {
            for row in 0..ROWS {
                let value = grain(step as f32 * 0.37, row as f32, 0xC0FFEE);
                assert!((0.0..=1.0).contains(&value), "{value} out of range");
            }
        }
    }

    /// A wet brush is loaded at the landing and dries as it travels, so the
    /// field's mean should read wetter over its first columns than its last.
    #[test]
    fn the_field_dries_out_along_its_length() {
        let mut rng = Rng::new(906);
        let (_, mean) = build(&mut rng);
        let head: f32 = mean[..60].iter().map(|&v| dequantize(v)).sum::<f32>() / 60.0;
        let tail: f32 = mean[mean.len() - 60..]
            .iter()
            .map(|&v| dequantize(v))
            .sum::<f32>()
            / 60.0;
        assert!(
            head > tail,
            "field did not dry out: head {head:.2}, tail {tail:.2}"
        );
    }
}
