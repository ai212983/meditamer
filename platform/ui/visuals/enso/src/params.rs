//! What makes one ensō differ from the next, and the tables its stroke is
//! rendered from.
//!
//! Every field below traces back to one of two things a brush actually does:
//! geometry -- where the bundle is, how wide it sits, whether a given bristle
//! is touching the paper at all -- driven by a pressure curve along the
//! stroke, and ink -- what each bristle lays down as it dries, read as a
//! coverage field indexed by position across the brush and position along the
//! stroke. Nothing here is tiled, spliced or stretched across a junction,
//! because there are no junctions: both indices are continuous.
//!
//! The pressure curve and the ink field are the two things this module spends
//! its parameter budget precomputing into fixed-size tables, so that
//! [`crate::stroke::Stroke::render`] never calls `powf`, `asinf` or the grain
//! hash while walking a rib -- only linear interpolation into what is baked
//! here once per stroke. See each table's field doc for what it replaces.

use core::f32::consts::{PI, TAU};

use libm::{floorf, powf, sinf};

use crate::curve::smoothstep;
use crate::ink::{self, ROWS, S_SAMPLES};
use crate::rng::Rng;

/// One low-frequency term of the ring's wobble around a true circle.
#[derive(Clone, Copy, Debug)]
pub struct Harmonic {
    /// Cycles per turn.
    pub cycles: f32,
    /// Amplitude as a fraction of the base radius.
    pub amplitude: f32,
    pub phase: f32,
}

/// How the brush enters the paper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Entry {
    /// 藏鋒, concealed tip. The brush arrives already bearing down, so the
    /// mark begins at most of its width and is rounded off *behind* the
    /// contact point rather than coming to a point.
    Concealed,
    /// 露鋒, exposed tip. The brush lands on its tip and the belly rolls down
    /// as the stroke gets underway, so the mark genuinely starts at nothing.
    Exposed,
}

/// How the brush leaves the paper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Exit {
    /// 止め, "stop". The brush releases abruptly and keeps its width.
    Stop,
    /// 払い, "sweep". The brush bleeds away to a point while still moving.
    Sweep,
    /// 跳ね, "jump". The stroke narrows to a point and hooks away from its own
    /// path as the brush leaves.
    Flick,
}

/// Bins the rim-fade table spans `|u|` (offset across the brush, as a
/// fraction of the current half-width) with. Coarse on purpose: the curve it
/// samples is a single smoothstep, so sixty-four steps already interpolate to
/// well under a pixel of error at any stroke width this model draws.
const RIM_BINS: usize = 64;

#[derive(Clone)]
pub struct EnsoParams {
    /// Always the canvas's own centre -- unlike the rest of this crate's
    /// former model, the reference never jitters it.
    pub center: (f32, f32),
    pub radius: f32,
    /// Widest the stroke ever gets, in pixels, before pressure and weight.
    pub width: f32,
    pub start: f32,
    /// Total turn, in radians. Kept close to one full turn -- see `from_seed`.
    pub sweep: f32,
    /// `+1.0` or `-1.0`.
    pub direction: f32,
    pub ecc: f32,
    pub tilt: f32,
    pub wobble: [Harmonic; 2],
    /// Stroke weight at zero progress, as a fraction of its final weight. The
    /// whole ring thickens toward 1.0 as the session runs, so every part of
    /// the mark keeps changing rather than freezing once the brush has
    /// passed.
    pub weight_start: f32,
    /// Ink coverage at zero progress, as a fraction of its final value, for
    /// the same reason as `weight_start`.
    pub ink_start: f32,
    /// How far the brush turns in the hand over the whole stroke, in whole
    /// widths of the bundle.
    ///
    /// A round brush is rotationally symmetric, so turning it changes nothing
    /// geometrically -- but it does carry each bristle around, and without
    /// this every dry bristle would stay pinned to the same side of the ring
    /// for the entire circle, leaving one edge watery and the other dry the
    /// whole way round.
    pub roll: f32,
    /// How far in from the rim the mark fades out, as a fraction of its
    /// half-width -- the ink spreading into the paper at the brush's edge.
    pub bleed: f32,
    /// How far a `Flick` hooks off the circle, in pixels. Signed, and zero for
    /// every other exit.
    pub flick: f32,
    /// How much of the entry cap's rounding sits behind the true landing, as
    /// a fraction of the half-width. Zero for an exposed entry, which starts
    /// on the tip and has nothing to round off behind it.
    pub plant: f32,
    pub entry: Entry,
    pub exit: Exit,

    /// Pressure at `s = 0`, raw (not yet raised to the 0.62 that turns
    /// pressure into a width fraction). The entry cap's rounding needs this
    /// on its own -- it computes its own quarter-circle taper of pressure
    /// behind the landing, and only then applies the same 0.62.
    press0: f32,
    /// `pressure(s).powf(0.62)` at each of [`S_SAMPLES`] positions along the
    /// stroke, so [`crate::stroke::Stroke`] can turn pressure into a width
    /// fraction with a lookup and a lerp instead of a `powf` per rib. The
    /// exponent itself is the arcsine contact rule's inverse relationship
    /// between width and reach -- see `u_max` -- read backwards: pressure
    /// widens the mark a little faster than linearly.
    press_pow: [f32; S_SAMPLES],
    /// The contact rule, inverted and precomputed: the largest `|u|` (offset
    /// across the brush, as a fraction of the current half-width) that
    /// pressure at this position can reach.
    ///
    /// The rule itself is that an outer bristle needs more pressure than a
    /// central one to touch down -- `need = (2/pi) * asin(|u|)`, touching once
    /// `pressure >= need`. Solved for `|u|` instead of solved for `need`, that
    /// same rule becomes `|u| <= sin(pressure * pi/2)`, which turns a contact
    /// test from an `asin` against a threshold into a comparison against a
    /// number already sitting in a table.
    u_max: [f32; S_SAMPLES],
    /// The ink field: coverage per bristle against position along the stroke,
    /// [`ROWS`] positions across the brush by [`S_SAMPLES`] along it, with
    /// grain already multiplied in -- see `ink::apply_grain`.
    ink: [[u8; S_SAMPLES]; ROWS],
    /// Column means of the field *before* grain, for the tip's convergence
    /// blend -- see `crate::stroke`.
    ink_mean: [u8; S_SAMPLES],
    /// `1.0 - smoothstep(1.0 - bleed, 1.0, edge)` at [`RIM_BINS`] steps of
    /// `edge` over `0.0..=1.0`, so the rim fade is a lookup rather than a
    /// smoothstep per sample. `bleed` is fixed for the whole stroke, which is
    /// what makes this table possible.
    rim: [f32; RIM_BINS],
}

impl EnsoParams {
    /// Draws one ensō for `seed`, sized to a square canvas of `canvas`
    /// pixels.
    ///
    /// The pixel ranges below are the reference's own, measured against its
    /// 600 px canvas; scaling them by `canvas / 600` is this crate's own
    /// addition; the reference does not do it, on the assumption that Its
    /// image is that fixed size.
    pub fn from_seed(seed: u32, canvas: f32) -> Self {
        let mut rng = Rng::new(seed);
        let scale = canvas / 600.0;

        // The ink field is built first, matching the reference's own order,
        // and before the two size parameters (`radius`, `width`) it will
        // later be indexed against -- it does not need either yet.
        let (mut ink_field, ink_mean) = ink::build(&mut rng);

        let entry = if rng.flip() {
            Entry::Concealed
        } else {
            Entry::Exposed
        };
        let exit = match rng.next_u32() % 3 {
            0 => Exit::Stop,
            1 => Exit::Sweep,
            _ => Exit::Flick,
        };

        // The ends are entirely this curve: a concealed entry plants the
        // brush almost at once, an exposed one lands on the tip and rolls
        // down; a stop releases abruptly and keeps its width, a sweep bleeds
        // away to a point.
        let ent = if entry == Entry::Concealed {
            rng.range(0.012, 0.030)
        } else {
            rng.range(0.05, 0.11)
        };
        let plant = if entry == Entry::Concealed {
            rng.range(0.70, 0.95)
        } else {
            0.0
        };
        // The reference draws all three exit lengths unconditionally -- they
        // sit as values in one dict literal, all evaluated before any of them
        // is selected -- and only then picks the one `exit` names. Replicated
        // here so the same seed keeps drawing the same numbers for the same
        // reasons, even though only one of the three is ever used.
        let ext_stop = rng.range(0.012, 0.030);
        let ext_sweep = rng.range(0.11, 0.22);
        let ext_flick = rng.range(0.05, 0.10);
        let ext = match exit {
            Exit::Stop => ext_stop,
            Exit::Sweep => ext_sweep,
            Exit::Flick => ext_flick,
        };

        let body_k1 = 1.0 + (rng.next_u32() % 2) as f32;
        let body_a1 = rng.range(0.10, 0.20);
        let body_ph1 = rng.range(0.0, TAU);
        let body_k2 = 2.0 + (rng.next_u32() % 3) as f32;
        let body_a2 = rng.range(0.04, 0.09);
        let body_ph2 = rng.range(0.0, TAU);

        let pressure_at = |s: f32| -> f32 {
            let mut rise = smoothstep(0.0, ent, s);
            rise = plant + (1.0 - plant) * rise;
            // `1 - smoothstep(1-ext, 1, s)`, algebraically: smoothstep is
            // symmetric under `t -> 1-t` (`f(1-t) = 1-f(t)`), and this is that
            // identity applied to the reference's own
            // `clip((1-s)/ext,0,1)`, eased the same way.
            let fall = 1.0 - smoothstep(1.0 - ext, 1.0, s);
            let body = 1.0
                + body_a1 * sinf(s * TAU * body_k1 + body_ph1)
                + body_a2 * sinf(s * TAU * body_k2 + body_ph2);
            (rise * fall * body.clamp(0.4, 1.6)).clamp(0.0, 1.0)
        };

        let mut press_pow = [0.0f32; S_SAMPLES];
        let mut u_max = [0.0f32; S_SAMPLES];
        for col in 0..S_SAMPLES {
            let pressure = pressure_at(ink::column_s(col));
            press_pow[col] = powf(pressure, 0.62);
            u_max[col] = sinf((pressure / 0.97).min(1.0) * (PI / 2.0));
        }
        let press0 = pressure_at(0.0);

        let radius = scale * rng.range(184.0, 200.0);
        let width = scale * rng.range(48.0, 72.0);
        let start = rng.range(-190.0, -60.0).to_radians();
        // Kept close to a full turn. Much past it and the tail runs a long
        // way over its own landing, which at one bit is a flat overlap
        // rather than ink over ink.
        let sweep = rng.range(348.0, 363.0).to_radians();
        let direction = if rng.flip() { 1.0 } else { -1.0 };
        let ecc = rng.range(-0.014, 0.014);
        let tilt = rng.range(-0.30, 0.30);
        let wobble = [
            Harmonic {
                cycles: 2.0,
                amplitude: rng.range(0.002, 0.006),
                phase: rng.range(0.0, TAU),
            },
            Harmonic {
                cycles: 3.0,
                amplitude: rng.range(0.001, 0.004),
                phase: rng.range(0.0, TAU),
            },
        ];
        let weight_start = rng.range(0.45, 0.65);
        let ink_start = rng.range(0.70, 0.86);
        let grain_strength = rng.range(0.12, 0.26);
        let grain_seed = rng.next_u32();
        let roll = rng.range(0.20, 0.75) * if rng.flip() { 1.0 } else { -1.0 };
        let bleed = rng.range(0.14, 0.30);
        let flick = if exit == Exit::Flick {
            rng.range(0.10, 0.26)
                * if rng.next_u32().is_multiple_of(5) {
                    1.0
                } else {
                    -1.0
                }
        } else {
            0.0
        };

        // Grain is baked in last, once the geometry it is keyed against
        // (arc length, which needs `sweep` and `radius`) is known -- see
        // `ink::apply_grain`.
        ink::apply_grain(&mut ink_field, grain_strength, grain_seed, sweep * radius);

        let mut rim = [0.0f32; RIM_BINS];
        for (bin, value) in rim.iter_mut().enumerate() {
            let edge = bin as f32 / (RIM_BINS - 1) as f32;
            *value = 1.0 - smoothstep(1.0 - bleed, 1.0, edge);
        }

        Self {
            center: (canvas * 0.5, canvas * 0.5),
            radius,
            width,
            start,
            sweep,
            direction,
            ecc,
            tilt,
            wobble,
            weight_start,
            ink_start,
            roll,
            bleed,
            flick,
            plant,
            entry,
            exit,
            press0,
            press_pow,
            u_max,
            ink: ink_field,
            ink_mean,
            rim,
        }
    }

    /// How far the ink can possibly reach from the centre, worst case over
    /// the whole stroke. Used to keep the drawing inside its canvas.
    pub fn outer_extent(&self) -> f32 {
        let wobble: f32 = self.wobble.iter().map(|h| h.amplitude).sum();
        let ring = self.radius * (1.0 + self.ecc.abs() + wobble);
        // Pressure is clipped to `0.0..=1.0` and weight never exceeds 1.0, so
        // the half-width itself never exceeds half the nominal width.
        let half_max = self.width * 0.5;
        let hook_max = libm::fabsf(self.flick) * self.width;
        ring + half_max + hook_max
    }

    pub(crate) fn press_pow_at(&self, i0: usize, i1: usize, fi: f32) -> f32 {
        self.press_pow[i0] * (1.0 - fi) + self.press_pow[i1] * fi
    }

    pub(crate) fn u_max_at(&self, i0: usize, i1: usize, fi: f32) -> f32 {
        self.u_max[i0] * (1.0 - fi) + self.u_max[i1] * fi
    }

    pub(crate) fn press0(&self) -> f32 {
        self.press0
    }

    /// The field's ungrained column means, for the seed-906 comparison
    /// diagnostic in `crate::stroke`'s `probe` module -- this is what the
    /// reference's own `hist.mean(axis=0)` reports, since grain there is
    /// multiplied in later, per rendered sample, rather than baked into the
    /// field itself. Not for render-path use -- that goes through
    /// `ink_mean_at`'s interpolated lookup instead.
    #[cfg(test)]
    pub(crate) fn ink_mean_table_for_diagnostics(&self) -> &[u8; S_SAMPLES] {
        &self.ink_mean
    }

    /// Bilinear ink lookup at fractional row `row` (`0.0..=ROWS-1`) and the
    /// column pair `(i0, i1, fi)` from [`crate::ink::index_for`].
    pub(crate) fn ink_at(&self, row: f32, i0: usize, i1: usize, fi: f32) -> f32 {
        let r0 = floorf(row).max(0.0) as usize;
        let r1 = (r0 + 1).min(ROWS - 1);
        let fr = row - r0 as f32;
        let a = ink::read(&self.ink, r0, i0);
        let b = ink::read(&self.ink, r1, i0);
        let c = ink::read(&self.ink, r0, i1);
        let d = ink::read(&self.ink, r1, i1);
        (a * (1.0 - fr) + b * fr) * (1.0 - fi) + (c * (1.0 - fr) + d * fr) * fi
    }

    pub(crate) fn ink_mean_at(&self, i0: usize, i1: usize, fi: f32) -> f32 {
        let a = self.ink_mean[i0] as f32 * (1.0 / 255.0);
        let b = self.ink_mean[i1] as f32 * (1.0 / 255.0);
        a * (1.0 - fi) + b * fi
    }

    /// The rim fade at `|u|`, `u` already expressed as a fraction of the
    /// current half-width.
    pub(crate) fn rim_at(&self, u_abs: f32) -> f32 {
        let edge = u_abs.clamp(0.0, 1.0);
        let idx = edge * (RIM_BINS - 1) as f32;
        let i0 = idx as usize;
        let i1 = (i0 + 1).min(RIM_BINS - 1);
        let f = idx - i0 as f32;
        self.rim[i0] * (1.0 - f) + self.rim[i1] * f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: f32 = 600.0;

    #[test]
    fn a_seed_reproduces_its_parameters() {
        let first = EnsoParams::from_seed(4242, CANVAS);
        let second = EnsoParams::from_seed(4242, CANVAS);
        assert_eq!(first.radius, second.radius);
        assert_eq!(first.sweep, second.sweep);
        assert_eq!(first.direction, second.direction);
        assert_eq!(first.start, second.start);
        assert_eq!(first.ink, second.ink);
    }

    #[test]
    fn different_seeds_give_different_strokes() {
        let first = EnsoParams::from_seed(1, CANVAS);
        let second = EnsoParams::from_seed(2, CANVAS);
        assert!(
            first.radius != second.radius || first.start != second.start,
            "seeds 1 and 2 produced the same geometry"
        );
    }

    /// The invariant that matters more than any single range: whatever the
    /// seed, the ink has to land on the panel.
    #[test]
    fn every_seed_fits_inside_the_canvas() {
        for seed in 0..2000 {
            let params = EnsoParams::from_seed(seed, CANVAS);
            let reach = params.outer_extent();
            let margin = CANVAS * 0.5 - reach;
            assert!(
                margin > 0.0,
                "seed {seed} reaches {reach:.1} px from a {:.0} px centre, overflowing the canvas",
                CANVAS * 0.5
            );
        }
    }

    #[test]
    fn both_sweep_directions_occur() {
        let clockwise = (0..200)
            .filter(|seed| EnsoParams::from_seed(*seed, CANVAS).direction > 0.0)
            .count();
        assert!(
            (60..140).contains(&clockwise),
            "{clockwise} of 200 seeds went one way"
        );
    }

    #[test]
    fn every_entry_and_exit_form_occurs() {
        let mut concealed = 0;
        let mut exposed = 0;
        let mut stops = 0;
        let mut sweeps = 0;
        let mut flicks = 0;
        for seed in 0..600 {
            let p = EnsoParams::from_seed(seed, CANVAS);
            match p.entry {
                Entry::Concealed => concealed += 1,
                Entry::Exposed => exposed += 1,
            }
            match p.exit {
                Exit::Stop => stops += 1,
                Exit::Sweep => sweeps += 1,
                Exit::Flick => flicks += 1,
            }
        }
        assert!(concealed > 150 && exposed > 150, "{concealed} / {exposed}");
        assert!(
            stops > 100 && sweeps > 100 && flicks > 100,
            "stop {stops}, sweep {sweeps}, flick {flicks}"
        );
    }

    #[test]
    fn derived_parameters_stay_in_their_documented_ranges() {
        for seed in 0..1000 {
            let p = EnsoParams::from_seed(seed, CANVAS);
            assert!(p.radius > 0.0 && p.radius < CANVAS);
            assert!(p.width > 0.0 && p.width < CANVAS * 0.2);
            assert!((0.45..0.65).contains(&p.weight_start));
            assert!((0.70..0.86).contains(&p.ink_start));
            assert!((0.14..0.30).contains(&p.bleed));
            assert!(p.direction == 1.0 || p.direction == -1.0);
            assert!(
                p.sweep > TAU * 0.9 && p.sweep < TAU * 1.1,
                "seed {seed} sweeps {:.3} turns",
                p.sweep / TAU
            );
            if p.exit != Exit::Flick {
                assert_eq!(p.flick, 0.0);
            }
            if p.entry == Entry::Exposed {
                assert_eq!(p.plant, 0.0);
            }
        }
    }

    #[test]
    fn u_max_grows_with_pressure() {
        // The arcsine contact rule is monotone: more pressure can only reach
        // further across the brush, never less. Checked against the formula
        // directly, since pressure itself rises then falls along the stroke.
        let u_max_of = |pressure: f32| sinf((pressure / 0.97).min(1.0) * (PI / 2.0));
        let mut previous = u_max_of(0.0);
        let mut pressure = 0.0f32;
        while pressure <= 1.0 {
            let value = u_max_of(pressure);
            assert!(value >= previous - 1e-6, "dipped at pressure {pressure}");
            previous = value;
            pressure += 0.01;
        }
    }
}
