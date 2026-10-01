//! Shared area-light shadow masks (option 3): one projected-visibility
//! pass per row, reused by every receiver the compositor shades.
//!
//! The reference path ([`crate::render`]) traces both hand silhouettes per
//! tap per receiver plane: dial plus up to two painted hand heights. This
//! module computes the same per-tap union with the same intersection and
//! sampling arithmetic, but once per row into caller-owned open-tap counts
//! (dial, lower hand, upper hand), which
//! [`crate::compose::render_compose_row_with_masks`] then feeds the shared
//! composite body. Height order (only casters above the receiver and below
//! the light) and the `> 0.5` bilinear-alpha threshold are unchanged; there
//! is no blurred-mask multiplication anywhere.
//!
//! Work avoided, not time claimed: per (tap, caster, receiver) the row
//! precomputes the conservative projected sprite interval (bilinear reads
//! outside `[-1, w] x [-1, h]` are exactly zero, so skipping them cannot
//! change a count) and samples alpha only inside it. A degenerate or
//! non-finite projection falls back to the verbatim reference ray test,
//! never to skipping. The host additionally reuses its retained stationary
//! base outside the conservative affected interval
//! ([`row_affected_range`]).
//!
//! Footprint: no statics, no heap, no borrows held. The context owns plain
//! scalars (about 100 bytes plus the retained [`Prepared`]); every row
//! plane is caller-owned (`3 * width` bytes via
//! [`mask_row_scratch_len`]). The row pass also uses two 80-byte paint
//! bitmaps and six projection combinations on its stack; the scratch-length
//! helper counts only caller-owned planes. DRAM accounting stays with the caller per
//! the DRAM budget.

use super::assets::Hands;
use super::render::{bilinear_alpha, ray_hits_hand, to_sprite_with, Blocker, Point, Prepared};
use super::scene::ClockScene;

/// Caller-owned scratch for one mask row: three `width`-byte open-count
/// planes (dial, lower hand, upper hand), or `None` on overflow.
pub fn mask_row_scratch_len(width: usize) -> Option<usize> {
    width.checked_mul(3)
}

/// Paint-flag bits per row pass; covers every real frame (`width <= 600`)
/// with bounded stack. Wider rows skip the paint gate and evaluate every
/// plane (still exact, just less skipped).
const PAINT_BITS: usize = 640;

/// What one [`mask_row_counts`] call did. `pixels` is the shared prefix
/// actually filled; `alpha_queries` counts the bilinear alpha samples
/// really executed — two paint samples per pixel plus at most two
/// silhouette samples per tap per needed plane (dial always, a hand plane
/// only where that hand paints). The analytic ceiling is therefore
/// `pixels * (2 + taps * 6)`; rows the bounds or the paint gate empty sit
/// far below it. The ray reference issues up to the same ceiling, so the
/// two numbers validate avoided work without any timing claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaskStats {
    pub pixels: u32,
    pub alpha_queries: u32,
}

impl MaskStats {
    pub const ZERO: Self = Self {
        pixels: 0,
        alpha_queries: 0,
    };

    pub fn add(&mut self, other: Self) {
        self.pixels = self.pixels.saturating_add(other.pixels);
        self.alpha_queries = self.alpha_queries.saturating_add(other.alpha_queries);
    }
}

/// What one [`prepare_mask_row`] call did: the filled [`MaskStats`], the
/// conservative half-open affected column interval, and the work actually
/// executed. Pixels outside `affected_range` are guaranteed clean — no
/// hand coverage and dial visibility exactly 1 — so the shade pass may
/// skip them. `paint_queries` counts the paint-gate alpha samples really
/// executed (two per paint-checked pixel); `tap_iterations` counts the
/// per-tap pixel visits really executed. Clean rows report `(0, 0)` with
/// both counters zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaskRowWork {
    pub stats: MaskStats,
    pub affected_range: (usize, usize),
    pub paint_queries: u32,
    pub tap_iterations: u32,
}

impl MaskRowWork {
    pub const ZERO: Self = Self {
        stats: MaskStats::ZERO,
        affected_range: (0, 0),
        paint_queries: 0,
        tap_iterations: 0,
    };
}

/// Reusable per-frame mask projection: the clamped scene geometry, the
/// shared [`Prepared`] invariants, and the lower/upper height order the
/// composite paints in. Built once per frame; every row call reuses it.
#[derive(Clone, Copy)]
pub struct MaskContext {
    center_x: f32,
    center_y: f32,
    radius: f32,
    width: u32,
    height: u32,
    light_x: f32,
    light_y: f32,
    light_h: f32,
    lower_h: f32,
    upper_h: f32,
    lower_is_hour: bool,
    prep: Prepared,
}

impl MaskContext {
    /// Snapshot the frame. Takes the same unclamped scene and hands as the
    /// row renderers; clamping happens here exactly once, and the retained
    /// [`Prepared`] uses the identical expressions as the ray path.
    pub fn for_frame(scene: &ClockScene, hands: &Hands<'_>) -> Self {
        let scene = scene.clamped();
        let prep = Prepared::for_frame(&scene, hands);
        // Same order rule as the composite: the strictly higher hand
        // paints last; ties keep hour below minute. Heights are clamped
        // finite here, so `<=` states the tie rule directly.
        let lower_is_hour = scene.hour_h <= scene.minute_h;
        let (lower_h, upper_h) = if lower_is_hour {
            (scene.hour_h, scene.minute_h)
        } else {
            (scene.minute_h, scene.hour_h)
        };
        Self {
            center_x: scene.center_x,
            center_y: scene.center_y,
            radius: scene.radius,
            width: scene.width,
            height: scene.height,
            light_x: scene.light_x,
            light_y: scene.light_y,
            light_h: scene.light_h,
            lower_h,
            upper_h,
            lower_is_hour,
            prep,
        }
    }

    /// Frame width the context was built for.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Frame height the context was built for.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Area-light tap count (1..=[`MAX_SAMPLES`]).
    pub fn taps_n(&self) -> u8 {
        self.prep.taps_n
    }

    /// Whether the hour hand is the lower painted layer (ties: yes).
    pub fn lower_is_hour(&self) -> bool {
        self.lower_is_hour
    }

    /// The retained frame invariants for the composite body.
    pub(crate) fn prepared(&self) -> &Prepared {
        &self.prep
    }
}

/// One (caster, receiver-plane) projection for a single light tap: the
/// sprite-space lines `fx(x) = afx * x + bfx`, `fy(x) = afy * x + bfy`
/// across the row. `live` is false when the height order already rules the
/// caster out or the whole row misses the sprite (with 1-texel bilinear
/// support); only `live` combos sample. `whole` marks a degenerate
/// projection that must run the verbatim reference test per pixel.
///
/// `t` and `qy` hoist the row-invariant half of the reference ray test
/// with its exact expressions (`t` from the height division, `qy` from
/// `py` at setup); only `qx` still varies per column. The affine lines
/// above stay a conservative skip guard — never the alpha decision.
#[derive(Clone, Copy)]
struct Combo<'a, 'b> {
    blocker: Blocker<'a, 'b>,
    light: Point,
    recv_z: f32,
    t: f32,
    qy: f32,
    afx: f32,
    bfx: f32,
    afy: f32,
    bfy: f32,
    sprite_w: f32,
    sprite_h: f32,
    live: bool,
    whole: bool,
}

/// Receiver dial-space y for screen row `y`.
fn row_py(ctx: &MaskContext, y: u32) -> f32 {
    (y as f32 - ctx.center_y) / ctx.radius
}

/// Receiver dial-space x for screen column `x`.
fn col_px(ctx: &MaskContext, x: usize) -> f32 {
    (x as f32 - ctx.center_x) / ctx.radius
}

/// Height-order eligibility shared with the reference ray test: the caster
/// must float strictly above the receiver plane and strictly below the tap.
fn eligible(caster_h: f32, recv_z: f32, light_z: f32) -> bool {
    caster_h > recv_z && light_z > caster_h
}

/// Build one combo: height-order eligibility, then the row lines from the
/// reference ray expressions (`t`, `q`, sprite mapping).
fn combo_for<'a, 'b>(
    blocker: Blocker<'a, 'b>,
    light: Point,
    recv_z: f32,
    ctx: &MaskContext,
    py: f32,
    row_len: usize,
) -> Combo<'a, 'b> {
    let mut combo = Combo {
        blocker,
        light,
        recv_z,
        t: 0.0,
        qy: 0.0,
        afx: 0.0,
        bfx: 0.0,
        afy: 0.0,
        bfy: 0.0,
        sprite_w: blocker.hand.width as f32,
        sprite_h: blocker.hand.height as f32,
        live: false,
        whole: false,
    };
    if !eligible(blocker.height, recv_z, light.z) {
        return combo;
    }
    let denom = light.z - recv_z;
    let t = (blocker.height - recv_z) / denom;
    if !t.is_finite() {
        // Degenerate projection: run the reference test per pixel.
        combo.live = true;
        combo.whole = true;
        return combo;
    }
    // Row-invariant half of the reference ray test, kept in its exact
    // expression and order (`qy` here is the decision value, not the
    // reassociated support form below). Setup runs once per row per
    // (tap, caster, plane); queries reuse these bits verbatim.
    combo.t = t;
    combo.qy = py + t * (light.y - py);
    let r = ctx.radius;
    // q(x) = (1-t) * p(x) + t * light, p(x) = ((x-cx)/r, py).
    let k = (1.0 - t) / r;
    let qx_c = (1.0 - t) * (-ctx.center_x / r) + t * light.x;
    let qy = (1.0 - t) * py + t * light.y;
    let (s, c) = (blocker.sin, blocker.cos);
    let scale = blocker.scale;
    combo.afx = c * k * scale;
    combo.bfx = blocker.hand.pivot_x + (qx_c * c + qy * s) * scale;
    combo.afy = -s * k * scale;
    combo.bfy = blocker.hand.pivot_y + (-qx_c * s + qy * c) * scale;
    if !combo.afx.is_finite()
        || !combo.bfx.is_finite()
        || !combo.afy.is_finite()
        || !combo.bfy.is_finite()
    {
        combo.live = true;
        combo.whole = true;
        return combo;
    }
    // Conservative row interval with 1-texel support; a row fully outside
    // the sprite can never block.
    let last = row_len.saturating_sub(1) as f32;
    let (fx0, fx1) = (combo.bfx, combo.afx * last + combo.bfx);
    let (fy0, fy1) = (combo.bfy, combo.afy * last + combo.bfy);
    combo.live = !(fx0.min(fx1) > combo.sprite_w
        || fx0.max(fx1) < -1.0
        || fy0.min(fy1) > combo.sprite_h
        || fy0.max(fy1) < -1.0);
    combo
}

/// Test one combo at column `x` with dial position (`px`, `py`). Outside
/// the sprite (with support) misses without sampling; inside samples the
/// original alpha arithmetic and thresholds at `> 0.5` like the reference.
/// Degenerate combos run the verbatim reference ray test. Every executed
/// sample — bounded or fallback — bumps `queries`.
fn combo_hit(combo: &Combo<'_, '_>, x: usize, px: f32, py: f32, queries: &mut u32) -> bool {
    if !combo.live {
        return false;
    }
    if combo.whole {
        *queries += 1;
        let recv = Point {
            x: px,
            y: py,
            z: combo.recv_z,
        };
        return ray_hits_hand(&combo.blocker, &recv, &combo.light);
    }
    let fx = combo.afx * x as f32 + combo.bfx;
    let fy = combo.afy * x as f32 + combo.bfy;
    if fx < -1.0 || fx > combo.sprite_w || fy < -1.0 || fy > combo.sprite_h {
        return false;
    }
    *queries += 1;
    // Prepared ray: eligibility passed at setup and `t`/`qy` were hoisted
    // with the exact reference expressions, so only `qx` is per-column.
    // The mapping, sampling, and `> 0.5` threshold below are the reference
    // body verbatim — no reassociation, no FMA, no changed distribution.
    // The affine lines above stay a conservative skip guard only.
    let qx = px + combo.t * (combo.light.x - px);
    let (fx, fy) = to_sprite_with(
        combo.blocker.hand.pivot_x,
        combo.blocker.hand.pivot_y,
        combo.blocker.sin,
        combo.blocker.cos,
        combo.blocker.scale,
        qx,
        combo.qy,
    );
    let w = combo.blocker.hand.width as i32;
    let h = combo.blocker.hand.height as i32;
    bilinear_alpha(combo.blocker.hand.alpha, w, h, fx, fy) > 0.5
}

/// The two hand blockers with frame heights resolved: the strictly
/// higher hand paints last and ties keep hour below minute, matching the
/// composite layer order.
fn frame_blockers<'a, 'b>(
    ctx: &MaskContext,
    hands: &'a Hands<'b>,
) -> (Blocker<'a, 'b>, Blocker<'a, 'b>) {
    let hour = Blocker {
        hand: &hands.hour,
        sin: ctx.prep.hour_sin,
        cos: ctx.prep.hour_cos,
        scale: ctx.prep.hour_scale,
        height: if ctx.lower_is_hour {
            ctx.lower_h
        } else {
            ctx.upper_h
        },
    };
    let minute = Blocker {
        hand: &hands.minute,
        sin: ctx.prep.min_sin,
        cos: ctx.prep.min_cos,
        scale: ctx.prep.minute_scale,
        height: if ctx.lower_is_hour {
            ctx.upper_h
        } else {
            ctx.lower_h
        },
    };
    (hour, minute)
}

/// Six combos for one tap: hour × {dial, lower, upper}, then minute × the
/// same planes, so the per-pixel loop unions index pairs (0,3), (1,4),
/// (2,5) for the SAME tap.
fn combos_for_tap<'a, 'b>(
    ctx: &MaskContext,
    hands: &'a Hands<'b>,
    light: Point,
    planes: &[f32; 3],
    py: f32,
    row_len: usize,
) -> [Combo<'a, 'b>; 6] {
    let (hour, minute) = frame_blockers(ctx, hands);
    [
        combo_for(hour, light, planes[0], ctx, py, row_len),
        combo_for(hour, light, planes[1], ctx, py, row_len),
        combo_for(hour, light, planes[2], ctx, py, row_len),
        combo_for(minute, light, planes[0], ctx, py, row_len),
        combo_for(minute, light, planes[1], ctx, py, row_len),
        combo_for(minute, light, planes[2], ctx, py, row_len),
    ]
}

/// Widen a half-open column interval by one pixel each side, clamped to
/// the row: float rounding in the analytic lines must never turn into a
/// skipped pixel. Widening only adds work, never changes a count.
fn widen1(iv: (usize, usize), end: usize) -> (usize, usize) {
    (iv.0.saturating_sub(1), iv.1.saturating_add(1).min(end))
}

/// Conservative paint interval for one blocker hand across the row: the
/// direct sprite mapping at fixed `py` as affine lines, intersected with
/// the bilinear support (`[-1, w] x [-1, h]` reads exactly zero outside,
/// so skipping the rest cannot change a paint bit), then widened.
fn blocker_paint_interval(
    blocker: &Blocker<'_, '_>,
    ctx: &MaskContext,
    py: f32,
    end: usize,
) -> Option<(usize, usize)> {
    let (afx, bfx, afy, bfy) = paint_lines(
        blocker.hand.pivot_x,
        blocker.hand.pivot_y,
        blocker.sin,
        blocker.cos,
        blocker.scale,
        ctx,
        py,
    );
    paint_interval(
        afx,
        bfx,
        afy,
        bfy,
        blocker.hand.width as f32,
        blocker.hand.height as f32,
        end,
    )
    .map(|iv| widen1(iv, end))
}

/// Conservative pixel interval for one tap from the tap's own combos: the
/// union of every live (caster, receiver-plane) projection, widened.
/// `None` when no combo can touch the row, so the tap loop is skipped;
/// the full row on any degenerate projection. No extra combo setup and
/// no per-tap plan array — the caller already built `combos`.
fn tap_shadow_interval(combos: &[Combo<'_, '_>; 6], end: usize) -> Option<(usize, usize)> {
    let mut acc: Option<(usize, usize)> = None;
    for combo in combos {
        if !combo.live {
            continue;
        }
        if combo.whole
            || !combo.afx.is_finite()
            || !combo.bfx.is_finite()
            || !combo.afy.is_finite()
            || !combo.bfy.is_finite()
        {
            return Some((0, end));
        }
        if let Some(iv) = paint_interval(
            combo.afx,
            combo.bfx,
            combo.afy,
            combo.bfy,
            combo.sprite_w,
            combo.sprite_h,
            end,
        ) {
            acc = union_range(acc, widen1(iv, end));
        }
    }
    acc
}

/// Sample one hand's paint coverage inside its own conservative interval
/// and set the corresponding bits. Uses the original `to_sprite_with` /
/// `bilinear` arithmetic and the `> 0.0` threshold; pixels outside the
/// interval read exactly zero, so skipping them cannot change a bit.
/// Returns the paint queries executed; also bumps `alpha` by the same.
fn paint_hand_bits(
    blocker: &Blocker<'_, '_>,
    ctx: &MaskContext,
    py: f32,
    interval: Option<(usize, usize)>,
    bits: &mut [u64],
    alpha: &mut u32,
) -> u32 {
    let Some((lo, hi)) = interval else {
        return 0;
    };
    let mut done = 0u32;
    let w = blocker.hand.width as i32;
    let h = blocker.hand.height as i32;
    for x in lo..hi {
        let px = col_px(ctx, x);
        let (fx, fy) = to_sprite_with(
            blocker.hand.pivot_x,
            blocker.hand.pivot_y,
            blocker.sin,
            blocker.cos,
            blocker.scale,
            px,
            py,
        );
        *alpha += 1;
        done += 1;
        if bilinear_alpha(blocker.hand.alpha, w, h, fx, fy) > 0.0 {
            bits[x / 64] |= 1u64 << (x % 64);
        }
    }
    done
}

/// One bounded row of open-tap counts for the three receiver planes, plus
/// the conservative affected interval and the work executed. Hand-plane
/// counts are evaluated only where that hand paints; elsewhere they are
/// fully open and are not a visibility map for the infinite plane. Writes
/// the shared prefix of the three slices and the frame width; a `y` past
/// the frame height is a no-op returning [`MaskRowWork::ZERO`]. Invalid
/// hands fill every count with the tap count (visibility 1, dial-only
/// composite) with a clean range and zero work counters. Counts per tap
/// union BOTH blockers before accumulating — never a per-caster opacity
/// product. The affected interval is accumulated during the same pass
/// from the same combos (no second [`row_affected_range`] setup); every
/// skipped pixel is outside every analytic bound, so the counts match the
/// unbounded row exactly.
pub fn prepare_mask_row(
    ctx: &MaskContext,
    hands: &Hands<'_>,
    y: u32,
    dial_open: &mut [u8],
    lower_open: &mut [u8],
    upper_open: &mut [u8],
) -> MaskRowWork {
    let n = dial_open
        .len()
        .min(lower_open.len())
        .min(upper_open.len())
        .min(ctx.width as usize);
    if y >= ctx.height || n == 0 {
        return MaskRowWork::ZERO;
    }
    let taps_n = ctx.prep.taps_n;
    if !hands.validate() {
        dial_open[..n].fill(taps_n);
        lower_open[..n].fill(taps_n);
        upper_open[..n].fill(taps_n);
        return MaskRowWork {
            stats: MaskStats {
                pixels: n as u32,
                alpha_queries: 0,
            },
            affected_range: (0, 0),
            paint_queries: 0,
            tap_iterations: 0,
        };
    }
    // Open counts start fully open; each blocked tap decrements. Planes
    // the paint gate rules out stay at the tap count (visibility 1, the
    // value the composite would have traced for unused planes anyway).
    dial_open[..n].fill(taps_n);
    lower_open[..n].fill(taps_n);
    upper_open[..n].fill(taps_n);
    let py = row_py(ctx, y);
    let mut stats = MaskStats {
        pixels: n as u32,
        alpha_queries: 0,
    };
    // Bounded paint gate: a hand plane's counts feed the composite only
    // where that hand paints (`sample.alpha > 0.0` in the reference), so
    // sample the paint alpha with the original mapping — but only inside
    // the conservative analytic paint union below — and skip dead planes
    // per tap. Bit-packed to keep the pass bounded-stack. Rows (and wide
    // rows past `PAINT_BITS`) without a paint footprint sample nothing.
    let (hour_b, minute_b) = frame_blockers(ctx, hands);
    let h_iv = blocker_paint_interval(&hour_b, ctx, py, n);
    let m_iv = blocker_paint_interval(&minute_b, ctx, py, n);
    let paint_union = match (h_iv, m_iv) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (Some(a), Some(b)) => union_range(Some(a), b),
    };
    let mut paint_h = [0u64; PAINT_BITS / 64];
    let mut paint_m = [0u64; PAINT_BITS / 64];
    let gate = n <= PAINT_BITS;
    let mut paint_queries = 0u32;
    if gate {
        paint_queries += paint_hand_bits(
            &hour_b,
            ctx,
            py,
            h_iv,
            &mut paint_h,
            &mut stats.alpha_queries,
        );
        paint_queries += paint_hand_bits(
            &minute_b,
            ctx,
            py,
            m_iv,
            &mut paint_m,
            &mut stats.alpha_queries,
        );
    }
    // Which plane index (1 = lower, 2 = upper) belongs to which hand.
    let hour_plane = if ctx.lower_is_hour { 1 } else { 2 };
    let planes = [0.0f32, ctx.lower_h, ctx.upper_h];
    let mut tap_iterations = 0u32;
    let mut affected = paint_union;
    for i in 0..taps_n {
        let (ox, oy) = ctx.prep.taps[usize::from(i)];
        let light = Point {
            x: ctx.light_x + ox,
            y: ctx.light_y + oy,
            z: ctx.light_h,
        };
        let combos = combos_for_tap(ctx, hands, light, &planes, py, n);
        // Projected per-tap bounds from the tap's own combos: pixels the
        // six projections cannot touch decide nothing and are skipped.
        let Some((lo, hi)) = tap_shadow_interval(&combos, n) else {
            continue;
        };
        affected = union_range(affected, (lo, hi));
        tap_iterations += (hi - lo) as u32;
        for x in lo..hi {
            let px = col_px(ctx, x);
            let q = &mut stats.alpha_queries;
            // Per-tap union of both blockers, one plane at a time. The
            // dial plane always counts; a hand plane only where it paints.
            if combo_hit(&combos[0], x, px, py, q) || combo_hit(&combos[3], x, px, py, q) {
                dial_open[x] -= 1;
            }
            let paints_h = !gate || (paint_h[x / 64] >> (x % 64)) & 1 != 0;
            let paints_m = !gate || (paint_m[x / 64] >> (x % 64)) & 1 != 0;
            let need_lower = if hour_plane == 1 { paints_h } else { paints_m };
            if need_lower
                && (combo_hit(&combos[1], x, px, py, q) || combo_hit(&combos[4], x, px, py, q))
            {
                lower_open[x] -= 1;
            }
            let need_upper = if hour_plane == 2 { paints_h } else { paints_m };
            if need_upper
                && (combo_hit(&combos[2], x, px, py, q) || combo_hit(&combos[5], x, px, py, q))
            {
                upper_open[x] -= 1;
            }
        }
    }
    MaskRowWork {
        stats,
        affected_range: affected.unwrap_or((0, 0)),
        paint_queries,
        tap_iterations,
    }
}

/// One row of open-tap counts for the three receiver planes. Hand-plane
/// counts are evaluated only where that hand paints; elsewhere they are
/// fully open and are not a visibility map for the infinite plane. Writes the
/// shared prefix of the three slices and the frame width; a `y` past the
/// frame height is a no-op returning [`MaskStats::ZERO`]. Invalid hands
/// fill every count with the tap count (visibility 1, dial-only composite).
/// Counts per tap union BOTH blockers before accumulating — never a
/// per-caster opacity product. Delegates to [`prepare_mask_row`] and
/// returns its stats; the counts are identical, only the range and work
/// counters are dropped.
pub fn mask_row_counts(
    ctx: &MaskContext,
    hands: &Hands<'_>,
    y: u32,
    dial_open: &mut [u8],
    lower_open: &mut [u8],
    upper_open: &mut [u8],
) -> MaskStats {
    prepare_mask_row(ctx, hands, y, dial_open, lower_open, upper_open).stats
}

/// Solve the conservative integer column interval with `a * x + b` inside
/// `[lo, hi]`, intersected with `[0, end)`. Returns `None` when empty.
/// Non-finite or flat-in-range lines cover the whole row (safe fallback).
fn solve_x_range(a: f32, b: f32, lo: f32, hi: f32, end: usize) -> Option<(usize, usize)> {
    if end == 0 {
        return None;
    }
    if !a.is_finite() || !b.is_finite() {
        return Some((0, end));
    }
    if a == 0.0 {
        if b >= lo && b <= hi {
            return Some((0, end));
        }
        return None;
    }
    let t0 = (lo - b) / a;
    let t1 = (hi - b) / a;
    let lo_x = libm::floorf(t0.min(t1)) as i64;
    let hi_x = libm::ceilf(t0.max(t1)) as i64;
    let lo_c = lo_x.clamp(0, end as i64) as usize;
    let hi_c = hi_x.clamp(0, end as i64) as usize;
    if lo_c >= hi_c {
        return None;
    }
    Some((lo_c, hi_c))
}

/// Union `b` into the accumulator.
fn union_range(acc: Option<(usize, usize)>, b: (usize, usize)) -> Option<(usize, usize)> {
    match acc {
        None => Some(b),
        Some((x0, x1)) => Some((x0.min(b.0), x1.max(b.1))),
    }
}

/// Paint lines for one hand across the row: the direct sprite mapping of
/// `px(x) = (x - cx)/r` at fixed `py` as `fx = afx * x + bfx` and friends.
fn paint_lines(
    pivot_x: f32,
    pivot_y: f32,
    s: f32,
    c: f32,
    scale: f32,
    ctx: &MaskContext,
    py: f32,
) -> (f32, f32, f32, f32) {
    let afx = c * scale / ctx.radius;
    let bfx = pivot_x + ((-ctx.center_x / ctx.radius) * c + py * s) * scale;
    let afy = -s * scale / ctx.radius;
    let bfy = pivot_y + ((ctx.center_x / ctx.radius) * s + py * c) * scale;
    (afx, bfx, afy, bfy)
}

/// Intersect the fx and fy column intervals; `None` when either is empty
/// or they do not overlap.
fn paint_interval(
    afx: f32,
    bfx: f32,
    afy: f32,
    bfy: f32,
    sprite_w: f32,
    sprite_h: f32,
    end: usize,
) -> Option<(usize, usize)> {
    let ix = solve_x_range(afx, bfx, -1.0, sprite_w, end)?;
    let iy = solve_x_range(afy, bfy, -1.0, sprite_h, end)?;
    let lo = ix.0.max(iy.0);
    let hi = ix.1.min(iy.1);
    if lo >= hi {
        return None;
    }
    Some((lo, hi))
}

/// Conservative half-open affected column interval for row `y`: the union
/// of both hands' paint footprints and every (tap, caster, receiver-plane)
/// shadow projection, each expanded to whole pixels. A pixel outside this
/// interval is guaranteed unaffected — no hand coverage and dial
/// visibility exactly 1 — so the host may reuse its retained stationary
/// base there. `(0, 0)` means the whole row is clean; degeneracy falls
/// back to the full row.
pub fn row_affected_range(
    ctx: &MaskContext,
    hands: &Hands<'_>,
    y: u32,
    width: usize,
) -> (usize, usize) {
    let end = width.min(ctx.width as usize);
    if y >= ctx.height || end == 0 || !hands.validate() {
        return (0, 0);
    }
    let py = row_py(ctx, y);
    let mut acc: Option<(usize, usize)> = None;
    // Hand paint footprints.
    let rots = [
        (
            &hands.hour,
            ctx.prep.hour_sin,
            ctx.prep.hour_cos,
            ctx.prep.hour_scale,
        ),
        (
            &hands.minute,
            ctx.prep.min_sin,
            ctx.prep.min_cos,
            ctx.prep.minute_scale,
        ),
    ];
    for (hand, s, c, scale) in rots {
        let (afx, bfx, afy, bfy) = paint_lines(hand.pivot_x, hand.pivot_y, s, c, scale, ctx, py);
        if let Some(iv) = paint_interval(
            afx,
            bfx,
            afy,
            bfy,
            hand.width as f32,
            hand.height as f32,
            end,
        ) {
            acc = union_range(acc, iv);
        }
    }
    // Shadow projections for every tap, caster, and receiver plane.
    let planes = [0.0f32, ctx.lower_h, ctx.upper_h];
    for i in 0..ctx.prep.taps_n {
        let (ox, oy) = ctx.prep.taps[usize::from(i)];
        let light = Point {
            x: ctx.light_x + ox,
            y: ctx.light_y + oy,
            z: ctx.light_h,
        };
        let combos = combos_for_tap(ctx, hands, light, &planes, py, end);
        for combo in combos {
            if !combo.live {
                continue;
            }
            if combo.whole
                || !combo.afx.is_finite()
                || !combo.bfx.is_finite()
                || !combo.afy.is_finite()
                || !combo.bfy.is_finite()
            {
                return (0, end);
            }
            if let Some(iv) = paint_interval(
                combo.afx,
                combo.bfx,
                combo.afy,
                combo.bfy,
                combo.sprite_w,
                combo.sprite_h,
                end,
            ) {
                acc = union_range(acc, iv);
            }
        }
    }
    acc.unwrap_or((0, 0))
}
