//! Walking the brush along its path and laying down ink.
//!
//! The reference this crate ports (`.scratch/extract/geoink.py`) is written
//! the other way around: it iterates every screen pixel, converts each one
//! back to a position along the stroke and across the brush with `atan2` and
//! `hypot`, and -- because that angle is only known modulo a full turn while
//! the stroke itself can run a little past one -- tries three candidate laps
//! per pixel to find which one the pixel actually belongs to. That is
//! correct, and it is about 1.8 seconds a frame on the target hardware.
//!
//! This instead walks the brush forward: a sequence of ribs -- short
//! cross-sections perpendicular to the path -- advancing along the stroke at
//! roughly half a pixel of arc per step, each one *writing* the pixels under
//! it. Forward rasterisation already knows which lap it is on, because it is
//! the one doing the sweeping, so the three-candidate search disappears
//! entirely; trigonometry moves from once per screen pixel to once per rib;
//! and where a long tail sweeps back over its own landing, the second pass
//! simply overdraws the first, which at one bit and monotonic ink is exactly
//! what should happen.
//!
//! Every frame is a *complete* stroke, redrawn from a cleared surface: the
//! circle's radius, wobble and ink field are all functions of absolute
//! position along the stroke as it will finally stand, not of how much of it
//! has been swept so far. What `progress` controls is how much of that fixed
//! timeline is currently live -- see [`Stroke::render`] -- plus two ramps
//! (`weight`, `ink_start`) that keep the *whole* mark thickening and
//! darkening as the session runs, so growth reads as one drawing developing
//! rather than a cursor tracing a finished one.

use libm::{cosf, fabsf, floorf, powf, sinf, sqrtf};
use raster::{Dither, Surface};

use crate::curve::lerp;
use crate::ink::{self, ROWS};
use crate::params::EnsoParams;

/// Ribs per pixel of centre-line arc. Consecutive ribs overlap slightly, which
/// costs a few percent of redundant writes and is far cheaper than the gaps
/// that appear on the inside of the curve without it.
const RIBS_PER_PIXEL: f32 = 2.0;

/// Samples across a rib per pixel of its width, for the same reason.
const SAMPLES_PER_PIXEL: f32 = 2.0;

/// A destination for one rib sample's coverage at one pixel.
///
/// The forward walk is identical whether the result lands on a one-bit
/// [`Surface`] through a [`Dither`] or, for measurement in tests, into a plain
/// coverage buffer -- only what happens to the number at the end differs. This
/// is what lets [`Stroke::render`] and the test-only coverage probe share every
/// line of geometry.
trait Sink {
    fn ink(&mut self, x: i32, y: i32, coverage: f32);
}

struct DitherSink<'a, S: Surface> {
    dither: Dither,
    surface: &'a mut S,
}

impl<S: Surface> Sink for DitherSink<'_, S> {
    fn ink(&mut self, x: i32, y: i32, coverage: f32) {
        // Only ever adds ink, never removes it -- see `raster::dither` and
        // `docs/references/display-refresh.md` for why the panel's partial
        // waveform requires that.
        if self.dither.inks(coverage, x, y) {
            self.surface.set(x, y, true);
        }
    }
}

pub struct Stroke {
    params: EnsoParams,
}

impl Stroke {
    pub fn new(params: EnsoParams) -> Self {
        Self { params }
    }

    /// Draws the ensō for `seed` on a square canvas of `canvas` pixels.
    pub fn from_seed(seed: u32, canvas: f32) -> Self {
        Self::new(EnsoParams::from_seed(seed, canvas))
    }

    pub fn params(&self) -> &EnsoParams {
        &self.params
    }

    /// Draws the whole stroke as it stands at `progress`, a value in
    /// `0.0..=1.0`.
    ///
    /// The surface must be cleared first -- this only ever sets ink, and a
    /// previous frame left underneath would show through as the union of the
    /// two. [`Stroke::bounds`] gives the region worth clearing.
    pub fn render(&self, progress: f32, dither: Dither, surface: &mut impl Surface) {
        let mut sink = DitherSink { dither, surface };
        self.walk(progress, &mut sink);
    }

    /// The region [`Stroke::render`] can touch, as `(x0, y0, x1, y1)` with the
    /// far edges exclusive. Constant across progress, so a caller can clear
    /// once and re-render into the same box.
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let reach = self.params.outer_extent() + 1.0;
        let (cx, cy) = self.params.center;
        (
            floorf(cx - reach) as i32,
            floorf(cy - reach) as i32,
            floorf(cx + reach) as i32 + 1,
            floorf(cy + reach) as i32 + 1,
        )
    }

    /// The shared walk. `progress` reveals the stroke along its own fixed
    /// timeline in three pieces, at most two of which do any per-rib
    /// trigonometry or `powf` -- everything else is a table lookup:
    ///
    /// * an entry cap, behind the true landing at `s = 0`, only when the
    ///   entry is concealed and only ever this wide -- it does not grow as the
    ///   session does;
    /// * the body, from the landing to wherever the brush currently is, pure
    ///   table lookups the whole way;
    /// * a tip taper, a brush-width-sized rounding of whatever is currently
    ///   the leading edge, present only while `progress < 1.0` and gone once
    ///   the stroke is finished.
    fn walk(&self, progress: f32, sink: &mut impl Sink) {
        let progress = progress.clamp(0.0, 1.0);
        if progress <= 0.0 {
            return;
        }
        let p = &self.params;
        let arc_per_s = p.sweep * p.radius;
        if arc_per_s < 1.0 {
            return;
        }

        // Both of these are physical widths (half the brush) converted to a
        // fraction of the stroke's own length, which is why they do not grow
        // as the ring does: a brush-width of rounding stays a brush-width of
        // rounding whether the arc behind it is five degrees or three hundred.
        let back = if p.plant > 0.0 {
            (p.width * 0.5 * p.plant) / arc_per_s
        } else {
            0.0
        };
        let tip = if progress < 1.0 {
            (p.width * 0.5) / arc_per_s
        } else {
            0.0
        };
        let s_hi = (progress + tip).min(1.0);
        if s_hi * arc_per_s < 1.0 {
            return;
        }

        let weight = lerp(p.weight_start, 1.0, progress);
        let ink_scale = p.ink_start + (1.0 - p.ink_start) * progress;

        if back > 0.0 {
            self.paint_back_cap(back, weight, ink_scale, sink);
        }
        self.paint_body(progress, weight, ink_scale, sink);
        if s_hi > progress {
            self.paint_tip_taper(progress, s_hi, tip, weight, ink_scale, sink);
        }
    }

    /// The interior: `s` from the landing to `progress`, at table-lookup cost
    /// per rib.
    fn paint_body(&self, progress: f32, weight: f32, ink_scale: f32, sink: &mut impl Sink) {
        let p = &self.params;
        let steps = self.step_count(progress);
        for step in 0..=steps {
            let s = progress * (step as f32 / steps as f32);
            let (i0, i1, fi) = ink::index_for(s);
            let press_pow = p.press_pow_at(i0, i1, fi);
            let u_max = p.u_max_at(i0, i1, fi);
            self.paint_rib(s, press_pow, u_max, weight, ink_scale, sink);
        }
    }

    /// The rounding behind the landing that a concealed entry needs: without
    /// it the start is pinned to a hard angular boundary, and as the ring
    /// gains weight the mark can spread sideways and forwards but never
    /// behind itself, inflating from a fixed corner instead of growing like a
    /// brush mark.
    ///
    /// `s` runs negative here, so the ink field's own `index_for` clips it to
    /// column zero -- the cap borrows column zero's texture and simply
    /// positions it behind the landing, which is what the reference does too.
    fn paint_back_cap(&self, back: f32, weight: f32, ink_scale: f32, sink: &mut impl Sink) {
        let p = &self.params;
        let steps = self.step_count_for(back);
        for step in 0..=steps {
            let s = -back + back * (step as f32 / steps as f32);
            let before = (-s / back).clamp(0.0, 1.0);
            let press_raw = p.press0() * sqrtf((1.0 - before * before).max(0.0));
            let press_pow = powf(press_raw, 0.62);
            let u_max = sinf((press_raw / 0.97).min(1.0) * (core::f32::consts::PI / 2.0));
            self.paint_rib(s, press_pow, u_max, weight, ink_scale, sink);
        }
    }

    /// The brush's contact patch has extent along the stroke as well as
    /// across it, so a stroke caught mid-motion does not end on a straight
    /// line: this carries the mark a half-width past wherever `progress`
    /// currently is and lets pressure fall to nothing over that distance, by
    /// the same contact rule as everything else, with nothing stamped.
    ///
    /// `press_pow` at a table point is already `pressure(s).powf(0.62)`, and
    /// this taper multiplies pressure itself by a quarter-circle factor
    /// `sqrt(1 - past^2)` before that power is taken -- but
    /// `(x * taper).powf(0.62) == x.powf(0.62) * taper.powf(0.62)`, so the
    /// width term only needs one more `powf`, of the taper alone, applied to
    /// the value already sitting in the table. The contact term does not
    /// factor the same way -- `asin` of a product is not a product of
    /// `asin`s -- so it alone needs the raw pressure back, recovered with the
    /// table's own inverse power. Both `powf` calls are confined to this taper
    /// zone, which is at most a brush half-width of arc.
    fn paint_tip_taper(
        &self,
        progress: f32,
        s_hi: f32,
        tip: f32,
        weight: f32,
        ink_scale: f32,
        sink: &mut impl Sink,
    ) {
        let p = &self.params;
        let steps = self.step_count_for(s_hi - progress);
        for step in 0..=steps {
            let s = progress + (s_hi - progress) * (step as f32 / steps as f32);
            let (i0, i1, fi) = ink::index_for(s);
            let press_pow_lookup = p.press_pow_at(i0, i1, fi);
            let past = ((s - progress) / tip).clamp(0.0, 1.0);
            let taper = sqrtf((1.0 - past * past).max(0.0));
            let press_pow = press_pow_lookup * powf(taper, 0.62);
            let press_raw = powf(press_pow_lookup, 1.0 / 0.62) * taper;
            let u_max = sinf((press_raw / 0.97).min(1.0) * (core::f32::consts::PI / 2.0));
            self.paint_rib(s, press_pow, u_max, weight, ink_scale, sink);
        }
    }

    /// Rib count for a span of `s`, spaced by arc length so a short span is
    /// sampled as finely as a long one rather than smeared over a fixed
    /// budget.
    fn step_count_for(&self, span: f32) -> i32 {
        let arc = span.abs() * self.params.sweep * self.params.radius;
        ((arc * RIBS_PER_PIXEL) as i32).max(1)
    }

    fn step_count(&self, progress: f32) -> i32 {
        self.step_count_for(progress)
    }

    /// Where the brush centre sits at absolute position `s` along the
    /// stroke's own fixed timeline, and the unit direction its rib runs in.
    ///
    /// Eccentricity and wobble are small enough (see `EnsoParams::from_seed`)
    /// that treating this direction as exactly radial in pixel space -- rather
    /// than solving for the true normal of a wobbling ellipse -- stays well
    /// under a pixel of error, which is worth having: it means no square root
    /// here at all.
    fn centre_at(&self, theta: f32) -> ((f32, f32), (f32, f32)) {
        let p = &self.params;
        let mut wobble = 0.0;
        for harmonic in &p.wobble {
            wobble += harmonic.amplitude * sinf(harmonic.cycles * theta + harmonic.phase);
        }
        let ring = p.radius * (1.0 + wobble);

        let (cos_t, sin_t) = (cosf(theta), sinf(theta));
        let ex = ring * (1.0 + p.ecc) * cos_t;
        let ey = ring * (1.0 - p.ecc) * sin_t;
        let (cos_tilt, sin_tilt) = (cosf(p.tilt), sinf(p.tilt));

        let axis = (
            cos_t * cos_tilt - sin_t * sin_tilt,
            cos_t * sin_tilt + sin_t * cos_tilt,
        );
        (
            (
                p.center.0 + ex * cos_tilt - ey * sin_tilt,
                p.center.1 + ex * sin_tilt + ey * cos_tilt,
            ),
            axis,
        )
    }

    /// Draws one rib at absolute position `s`, given the pressure-derived
    /// width (`press_pow`) and contact reach (`u_max`) it should use --
    /// either both looked up from the stroke's tables, or both recomputed for
    /// a taper zone. Everything from here down is the same regardless of
    /// which.
    #[allow(clippy::too_many_arguments)]
    fn paint_rib(
        &self,
        s: f32,
        press_pow: f32,
        u_max: f32,
        weight: f32,
        ink_scale: f32,
        sink: &mut impl Sink,
    ) {
        let p = &self.params;
        let theta = p.start + p.direction * p.sweep * s;
        let (centre, axis) = self.centre_at(theta);

        let half = (p.width * 0.5 * press_pow * weight).max(0.4);
        // A `Flick` pulls the path aside as the brush leaves, squared so the
        // departure accelerates rather than curving away evenly.
        let hook_t = ((s - 0.90) / 0.10).clamp(0.0, 1.0);
        let hook = p.flick * p.width * hook_t * hook_t;

        // As the brush lifts, its bristles come together: once the mark is
        // only a few pixels across they are no longer resolvable individually,
        // so sampling many of them aliases into a splay of needles at the
        // tail. Converge toward the bundle's average instead, which is what a
        // lifting brush leaves.
        let converge = (half / 5.0).clamp(0.0, 1.0);

        let (i0, i1, fi) = ink::index_for(s);
        let mean_ink = p.ink_mean_at(i0, i1, fi);

        let samples = ((half * SAMPLES_PER_PIXEL) as i32).max(1);
        for step in -samples..=samples {
            let u_frac = step as f32 / samples as f32;
            // Contact: an outer bristle needs more pressure to reach the paper
            // than a central one -- `u_max` is that rule already inverted, so
            // this is a comparison rather than an `asin`.
            if fabsf(u_frac) > u_max {
                continue;
            }
            let u = half * u_frac;

            // Reflected, not wrapped. Modulo would make a bristle reaching one
            // edge of the mark reappear instantly at the other, which is a
            // break in its track; a round brush turning in the hand carries a
            // bristle across the mark and back again, so the row index folds
            // at the ends instead.
            let walk = (u_frac * 0.5 + 0.5) + p.roll * s;
            let fold = fabsf(modulo(walk, 2.0) - 1.0);
            let row = ((1.0 - fold) * (ROWS - 1) as f32).clamp(0.0, (ROWS - 1) as f32);

            let ink_value = p.ink_at(row, i0, i1, fi);
            let mixed = mean_ink + (ink_value - mean_ink) * converge;
            let rim = p.rim_at(fabsf(u_frac));
            let coverage = (mixed * rim * ink_scale).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }

            let x = floorf(centre.0 + (u + hook) * axis.0 + 0.5) as i32;
            let y = floorf(centre.1 + (u + hook) * axis.1 + 0.5) as i32;
            sink.ink(x, y, coverage);
        }
    }
}

/// Python's `%`, which is always non-negative for a positive modulus, unlike
/// Rust's `%`, which keeps the sign of its left operand. The row fold below
/// depends on that: a negative `walk` has to land in the same place a
/// wrapped-around positive one would.
fn modulo(a: f32, m: f32) -> f32 {
    let r = a % m;
    if r < 0.0 {
        r + m
    } else {
        r
    }
}

/// Test-only support shared by `tests` and `probe` below: a coverage sink and
/// the oscillation metric, kept at module level (rather than nested in
/// `tests`) so the `#[ignore]`d diagnostic module can reach them too.
#[cfg(test)]
mod test_support {
    use super::*;
    use std::vec::Vec;

    /// A coverage sink for tests: the continuous field the reference computes
    /// before dithering, which is what oscillation is measured against --
    /// dithering itself introduces its own pixel-to-pixel jump by design and
    /// would swamp the thing this metric is trying to catch.
    pub(super) struct CoverageSink {
        pub(super) width: i32,
        pub(super) height: i32,
        pub(super) cov: Vec<f32>,
    }

    impl CoverageSink {
        pub(super) fn new(width: i32, height: i32) -> Self {
            Self {
                width,
                height,
                cov: std::vec![0.0f32; (width * height) as usize],
            }
        }
    }

    impl Sink for CoverageSink {
        fn ink(&mut self, x: i32, y: i32, coverage: f32) {
            if x < 0 || y < 0 || x >= self.width || y >= self.height {
                return;
            }
            let idx = (y * self.width + x) as usize;
            if coverage > self.cov[idx] {
                self.cov[idx] = coverage;
            }
        }
    }

    pub(super) fn render_coverage(seed: u32, progress: f32, size: i32) -> CoverageSink {
        let stroke = Stroke::from_seed(seed, size as f32);
        let mut sink = CoverageSink::new(size, size);
        stroke.walk(progress, &mut sink);
        sink
    }

    /// Mean and 95th-percentile of `|2*c[i] - c[i-1] - c[i+1]|` over both axes
    /// of the continuous coverage field, restricted to pixels the stroke
    /// actually reaches.
    ///
    /// This is the metric that caught the moire the field's row count and the
    /// bristle-history halving in the reference both exist to fix -- see
    /// `crate::ink`'s module doc. A stroke rendered pixel by pixel with the
    /// wrong resolution somewhere looks fine in a screenshot and fails this by
    /// a wide margin, which is why it is worth keeping as a standing test
    /// rather than only as the diagnostic that first found the problem.
    pub(super) fn oscillation(cov: &[f32], width: i32, height: i32) -> (f32, f32) {
        let mut samples: Vec<f32> = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let c = cov[idx];
                if c <= 0.0 {
                    continue;
                }
                if x > 0 && x < width - 1 {
                    let l = cov[idx - 1];
                    let r = cov[idx + 1];
                    samples.push(fabsf(2.0 * c - l - r));
                }
                if y > 0 && y < height - 1 {
                    let u = cov[idx - width as usize];
                    let d = cov[idx + width as usize];
                    samples.push(fabsf(2.0 * c - u - d));
                }
            }
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        let p95 = samples[((samples.len() as f32 * 0.95) as usize).min(samples.len() - 1)];
        (mean, p95)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{oscillation, render_coverage};
    use super::*;
    use raster::BitCanvas;

    const CANVAS: i32 = 240;
    const BYTES: usize = BitCanvas::bytes_for(CANVAS, CANVAS);
    const MAX_PROBE: i32 = 420;

    fn render_into(seed: u32, progress: f32, size: i32, bits: &mut [u8]) -> u32 {
        let stroke = Stroke::from_seed(seed, size as f32);
        let mut canvas = BitCanvas::new(size, size, bits).expect("canvas");
        stroke.render(progress, Dither::Gradient, &mut canvas);
        canvas.ink_count()
    }

    fn ink_fraction(seed: u32, size: i32) -> f32 {
        let mut bits = [0u8; BitCanvas::bytes_for(MAX_PROBE, MAX_PROBE)];
        render_into(seed, 1.0, size, &mut bits) as f32 / (size * size) as f32
    }

    #[test]
    fn a_seed_reproduces_its_drawing() {
        let mut first = [0u8; BYTES];
        let mut second = [0u8; BYTES];
        render_into(99, 0.6, CANVAS, &mut first);
        render_into(99, 0.6, CANVAS, &mut second);
        assert_eq!(first, second);
    }

    #[test]
    fn different_seeds_draw_differently() {
        let mut first = [0u8; BYTES];
        let mut second = [0u8; BYTES];
        render_into(1, 1.0, CANVAS, &mut first);
        render_into(2, 1.0, CANVAS, &mut second);
        assert_ne!(first, second);
    }

    #[test]
    fn ink_grows_with_progress() {
        for seed in 0..40 {
            let mut bits = [0u8; BYTES];
            let mut previous = 0;
            for step in 1..=8 {
                bits.fill(0);
                let ink = render_into(seed, step as f32 / 8.0, CANVAS, &mut bits);
                assert!(
                    ink > previous,
                    "seed {seed} did not grow at step {step}: {ink} <= {previous}"
                );
                previous = ink;
            }
        }
    }

    #[test]
    fn zero_progress_draws_nothing() {
        let mut bits = [0u8; BYTES];
        assert_eq!(render_into(5, 0.0, CANVAS, &mut bits), 0);
    }

    #[test]
    fn bounds_contain_every_rendered_pixel() {
        for seed in 0..60 {
            let stroke = Stroke::from_seed(seed, CANVAS as f32);
            let (x0, y0, x1, y1) = stroke.bounds();
            let mut bits = [0u8; BYTES];
            let mut canvas = BitCanvas::new(CANVAS, CANVAS, &mut bits).expect("canvas");
            stroke.render(1.0, Dither::Gradient, &mut canvas);
            for y in 0..CANVAS {
                for x in 0..CANVAS {
                    if canvas.get(x, y) {
                        assert!(
                            x >= x0 && x < x1 && y >= y0 && y < y1,
                            "seed {seed} inked ({x}, {y}) outside bounds \
                             ({x0}, {y0})..({x1}, {y1})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn ink_fraction_is_roughly_canvas_independent() {
        for seed in 0..40 {
            let small = ink_fraction(seed, 180);
            let large = ink_fraction(seed, 420);
            assert!(
                (small - large).abs() < 0.03,
                "seed {seed} inked {:.1}% at 180px but {:.1}% at 420px",
                small * 100.0,
                large * 100.0
            );
        }
    }

    /// A stroke that covers most of the canvas, or almost none of it, is a
    /// broken parameter range rather than an unusual ensō.
    #[test]
    fn every_seed_inks_a_plausible_area() {
        for seed in 0..2000 {
            let fraction = ink_fraction(seed, CANVAS);
            assert!(
                (0.01..0.35).contains(&fraction),
                "seed {seed} inked {:.1}% of the canvas",
                fraction * 100.0
            );
        }
    }

    #[test]
    fn all_dither_modes_render_at_every_progress() {
        for dither in [Dither::None, Dither::Bayer4, Dither::Gradient] {
            for step in 0..=20 {
                let mut bits = [0u8; BYTES];
                let mut canvas = BitCanvas::new(CANVAS, CANVAS, &mut bits).expect("canvas");
                Stroke::from_seed(3, CANVAS as f32).render(step as f32 / 20.0, dither, &mut canvas);
            }
        }
    }

    /// See `test_support::oscillation`. The reference measures roughly 0.013
    /// mean and 0.043 p95 on its own per-pixel evaluation of seed 906; this
    /// port measures roughly 0.017-0.032 mean and 0.05-0.13 p95 across forty
    /// seeds at the same 600 px size (see `probe::seed_906_report` for the
    /// exact seed-906 numbers). Higher, and expected to be: forward
    /// rasterisation samples a rib's width at a fixed density in pixels
    /// rather than inverting an exact pixel grid, and several of this port's
    /// tables (`press_pow`, `u_max`, grain) commute a nonlinear step with the
    /// same linear interpolation the reference applies before it, not after.
    /// The bound below asserts with headroom over what this port actually
    /// measures, not over the reference's own number -- the two are different
    /// rasterisations of the same model and are not expected to match to the
    /// last bit.
    #[test]
    fn oscillation_stays_bounded() {
        // The panel this model actually targets, and the size the reference's
        // own measurement was taken at -- oscillation is a function of how
        // fine the pixel grid is relative to the stroke, so a different size
        // is not a fair comparison either way.
        const SIZE: i32 = 600;
        let mut worst_mean = 0.0f32;
        let mut worst_p95 = 0.0f32;
        for seed in 0..40 {
            let sink = render_coverage(seed * 37 + 906, 1.0, SIZE);
            let (mean, p95) = oscillation(&sink.cov, SIZE, SIZE);
            worst_mean = worst_mean.max(mean);
            worst_p95 = worst_p95.max(p95);
        }
        assert!(
            worst_mean < 0.05,
            "oscillation mean {worst_mean:.4} exceeds bound"
        );
        assert!(
            worst_p95 < 0.16,
            "oscillation p95 {worst_p95:.4} exceeds bound"
        );
    }
}

/// Diagnostics for comparing this port against the Python reference. Not
/// assertions -- the two rasterise differently enough that matching to the
/// last bit is not the goal -- so this is `#[ignore]`d and only ever run by
/// hand.
///
/// ```sh
/// cargo test -p enso --lib seed_906_report -- --ignored --nocapture
/// ```
#[cfg(test)]
mod probe {
    use super::test_support::{oscillation, render_coverage};
    use super::*;
    use raster::BitCanvas;

    #[test]
    #[ignore = "diagnostic, not an assertion"]
    fn seed_906_report() {
        const SIZE: i32 = 600;
        let sink = render_coverage(906, 1.0, SIZE);
        let (mean, p95) = oscillation(&sink.cov, SIZE, SIZE);
        let touched = sink.cov.iter().filter(|&&c| c > 0.0).count();
        let total_inked: f32 = sink.cov.iter().filter(|&&c| c > 0.0).sum();
        let coverage_weighted = total_inked / (SIZE * SIZE) as f32;

        let mut bits = [0u8; BitCanvas::bytes_for(600, 600)];
        let dithered = {
            let mut canvas = BitCanvas::new(SIZE, SIZE, &mut bits).expect("canvas");
            Stroke::from_seed(906, SIZE as f32).render(1.0, Dither::Gradient, &mut canvas);
            canvas.ink_count()
        };

        let params = EnsoParams::from_seed(906, SIZE as f32);
        let ink_mean = params.ink_mean_table_for_diagnostics();
        let dq = |v: u8| v as f32 * (1.0 / 255.0);
        let first_60: f32 = ink_mean[..60].iter().copied().map(dq).sum::<f32>() / 60.0;
        let last_60: f32 = ink_mean[ink_mean.len() - 60..]
            .iter()
            .copied()
            .map(dq)
            .sum::<f32>()
            / 60.0;

        std::println!("seed 906 at {SIZE}px:");
        std::println!("  oscillation mean {mean:.4}, p95 {p95:.4}");
        std::println!(
            "  dithered ink fraction (Dither::Gradient, one bit): {:.2}%",
            dithered as f32 / (SIZE * SIZE) as f32 * 100.0
        );
        std::println!(
            "  pixels the stroke geometry touches at all: {touched} ({:.2}% of canvas), \
             coverage-weighted {:.2}%",
            touched as f32 / (SIZE * SIZE) as f32 * 100.0,
            coverage_weighted * 100.0
        );
        std::println!("  ink field mean, first 60 columns: {first_60:.3}");
        std::println!("  ink field mean, last 60 columns:  {last_60:.3}");
    }
}
