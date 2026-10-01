//! The shared CPU renderer: per-pixel lighting, height-ordered soft
//! shadows, composite, and output.
//!
//! All shading happens in linear light: sRGB albedo decodes on sample,
//! normal and specular maps are data (no gamma), and the composed linear
//! color encodes once to sRGB grayscale. Dithering applies exactly once
//! per screen pixel in [`render_surface`], anchored at output coordinates.
//!
//! Shadows come from a fixed deterministic area light: golden-angle spiral
//! taps over the light disk. Each tap ray-tests **both** hand silhouettes
//! and takes their union before averaging, so two overlapping blockers
//! never double-darken (independent blurred-opacity multiplication would).
//! Only hands above the receiver cast: the upper hand shadows the lower
//! one and the dial, never the reverse. Visibility scales direct diffuse
//! and specular; ambient stays flat.
//!
//! Everything here borrows; per-pixel state is a few floats on the stack,
//! so a device can stream rows without ever holding a frame.

use super::assets::Hands;
use super::dial::{dial_finish, HUB, TICK};
use super::scene::{ClockScene, MAX_SAMPLES};
use raster::Dither;
use raster::Surface;

/// sRGB texel to linear light.
fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        libm::powf((c + 0.055) / 1.055, 2.4)
    }
}

/// Linear light to sRGB, clamped to the unit range.
fn to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * libm::powf(c, 1.0 / 2.4) - 0.055
    }
}

/// One bilinear sprite sample: straight-alpha coverage, linear albedo,
/// world-rotated unit normal, and data specular.
struct HandSample {
    alpha: f32,
    albedo: (f32, f32, f32),
    normal: (f32, f32, f32),
    spec: f32,
}

/// Exact u8 to unit-float normalization: `BYTE_NORM[b]` is bit-identical
/// to `f32::from(b) / 255.0` for every byte (proven by
/// `normalization_table_matches_original_division_all256`). A 1024-byte
/// read-only table in flash (`.rodata`); no heap, DRAM, task, or channel
/// change. Mask-only experiment arm: only the shadow-mask alpha path
/// (`bilinear_alpha`) reads the table; material samples and the reference
/// ray test keep the original float divide through the `false` policy.
const fn build_byte_norm() -> [f32; 256] {
    let mut table = [0.0f32; 256];
    let mut i = 0usize;
    while i < 256 {
        table[i] = (i as f32) / 255.0;
        i += 1;
    }
    table
}

static BYTE_NORM: [f32; 256] = build_byte_norm();

/// Bilinear read of one byte channel of a packed sprite plane; outside the
/// sprite reads zero, which is the clipping rule (transparent, unlit,
/// non-blocking). `stride` is bytes per pixel, `ch` the channel offset, so
/// tightly packed gray/alpha planes use stride 1 and RGB planes stride 3.
fn bilinear_strided_with<const TABLE: bool>(
    map: &[u8],
    w: i32,
    h: i32,
    fx: f32,
    fy: f32,
    ch: usize,
    stride: usize,
) -> f32 {
    let x0 = libm::floorf(fx) as i32;
    let y0 = libm::floorf(fy) as i32;
    let tx = (fx - x0 as f32).clamp(0.0, 1.0);
    let ty = (fy - y0 as f32).clamp(0.0, 1.0);
    let at = |x: i32, y: i32| -> f32 {
        if x < 0 || y < 0 || x >= w || y >= h {
            0.0
        } else {
            let byte = map[((y as usize) * (w as usize) + (x as usize)) * stride + ch];
            if TABLE {
                BYTE_NORM[byte as usize]
            } else {
                f32::from(byte) / 255.0
            }
        }
    };
    let a = at(x0, y0);
    let b = at(x0 + 1, y0);
    let c = at(x0, y0 + 1);
    let d = at(x0 + 1, y0 + 1);
    a * (1.0 - tx) * (1.0 - ty) + b * tx * (1.0 - ty) + c * (1.0 - tx) * ty + d * tx * ty
}

/// Bilinear read of one byte channel of a packed sprite plane; outside the
/// sprite reads zero, which is the clipping rule (transparent, unlit,
/// non-blocking). `stride` is bytes per pixel, `ch` the channel offset, so
/// tightly packed gray/alpha planes use stride 1 and RGB planes stride 3.
/// Original normalization policy: plain float divide, as before the table.
fn bilinear_strided(map: &[u8], w: i32, h: i32, fx: f32, fy: f32, ch: usize, stride: usize) -> f32 {
    bilinear_strided_with::<false>(map, w, h, fx, fy, ch, stride)
}

/// Bilinear read of a tightly packed one-byte-per-pixel plane.
/// Original normalization policy: plain float divide (material samples and
/// the reference ray test).
pub(crate) fn bilinear(map: &[u8], w: i32, h: i32, fx: f32, fy: f32) -> f32 {
    bilinear_strided(map, w, h, fx, fy, 0, 1)
}

/// Bilinear read of a tightly packed one-byte-per-pixel plane for the
/// shadow-mask alpha path only: identical weights, order, and clipping,
/// with the byte normalization served from `BYTE_NORM`.
pub(crate) fn bilinear_alpha(map: &[u8], w: i32, h: i32, fx: f32, fy: f32) -> f32 {
    bilinear_strided_with::<true>(map, w, h, fx, fy, 0, 1)
}

/// Maps a dial-space point into one hand's sprite pixels: undo the hand's
/// clockwise rotation, scale dial units to sprite pixels, add the pivot.
/// Takes the cached rotation from [`Prepared`]; same expression and order
/// as the former per-call `libm::sinf`/`cosf` evaluation.
pub(crate) fn to_sprite_with(
    pivot_x: f32,
    pivot_y: f32,
    s: f32,
    c: f32,
    scale: f32,
    px: f32,
    py: f32,
) -> (f32, f32) {
    let qx = px * c + py * s;
    let qy = -px * s + py * c;
    (pivot_x + qx * scale, pivot_y + qy * scale)
}

/// Rotates a tangent-space normal XY by the hand's clockwise angle; the
/// out-of-plane component is untouched by an in-plane rotation.
/// Takes the cached rotation from [`Prepared`]; same expression and order
/// as the former per-call `libm::sinf`/`cosf` evaluation.
fn rotate_normal_with(nx: f32, ny: f32, s: f32, c: f32) -> (f32, f32) {
    (nx * c - ny * s, nx * s + ny * c)
}

/// One bilinear sprite sample with a cached hand rotation from [`Prepared`].
/// Same samples in the same order as the former per-call path; only the
/// sin/cos come from the caller.
fn sample_hand_with(
    hand: &super::assets::HandMaps<'_>,
    s: f32,
    c: f32,
    scale: f32,
    px: f32,
    py: f32,
) -> HandSample {
    let w = hand.width as i32;
    let h = hand.height as i32;
    let (fx, fy) = to_sprite_with(hand.pivot_x, hand.pivot_y, s, c, scale, px, py);
    let alpha = bilinear(hand.alpha, w, h, fx, fy);
    // Transparent texel: skip the hidden albedo/normal/specular samples
    // (and the normal normalization) entirely. Only an exactly-zero alpha
    // takes this path; any positive coverage — however faint — keeps the
    // original sampling below unchanged. The caller additionally skips
    // compositing when `alpha <= 0.0`, so this is purely work avoided.
    if alpha == 0.0 {
        return HandSample {
            alpha,
            albedo: (0.0, 0.0, 0.0),
            normal: (0.0, 0.0, 1.0),
            spec: 0.0,
        };
    }
    // Lossless gray albedo: the canonical source repeats the same gray in
    // all three channels, so one bilinear sample replicated three ways is
    // exactly what the RGB path computes (bilinear filtering is linear, and
    // identical inputs take identical float paths). Length decides the path;
    // a malformed slice never reaches here because invalid hands composite
    // dial-only (see `pixel_composite`).
    let (ar, ag, ab) = if hand.albedo_is_gray() {
        let g = bilinear(hand.albedo, w, h, fx, fy);
        (g, g, g)
    } else {
        (
            bilinear_strided(hand.albedo, w, h, fx, fy, 0, 3),
            bilinear_strided(hand.albedo, w, h, fx, fy, 1, 3),
            bilinear_strided(hand.albedo, w, h, fx, fy, 2, 3),
        )
    };
    // Normals filter as signed components, then renormalize. The stored
    // maps hold unsigned bytes around 127.5; green additionally negates
    // from stored-up into renderer y-down (see `decode_normal`).
    let signed =
        |ch: usize| -> f32 { bilinear_strided(hand.normal, w, h, fx, fy, ch, 3) * 2.0 - 1.0 };
    let (fr, fg, fb) = (signed(0), -signed(1), signed(2));
    let len = libm::sqrtf(fr * fr + fg * fg + fb * fb);
    let (lnx, lny, lnz) = if len > 1e-6 {
        (fr / len, fg / len, fb / len)
    } else {
        (0.0, 0.0, 1.0)
    };
    let (nx, ny) = rotate_normal_with(lnx, lny, s, c);
    HandSample {
        alpha,
        albedo: (to_linear(ar), to_linear(ag), to_linear(ab)),
        normal: (nx, ny, lnz),
        spec: bilinear(hand.spec, w, h, fx, fy),
    }
}

/// Deterministic area-light tap `i` of `n`: golden-angle spiral over the
/// light disk. Zero size collapses every tap to the light center.
fn light_tap(i: u8, n: u8, size: f32) -> (f32, f32) {
    if size <= 0.0 || n <= 1 {
        return (0.0, 0.0);
    }
    let f = (f32::from(i) + 0.5) / f32::from(n);
    let r = size * libm::sqrtf(f);
    let a = f32::from(i) * 2.399_963;
    (r * libm::cosf(a), r * libm::sinf(a))
}

/// A dial-space point: x/y in dial units from the center, z up.
#[derive(Clone, Copy)]
pub(crate) struct Point {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// One shadow caster: a hand's silhouette floating at its virtual height.
/// The rotation is a cached (sin, cos) pair from [`Prepared`], never a fresh
/// transcendental per ray; the mapping expression is unchanged.
#[derive(Clone, Copy)]
pub(crate) struct Blocker<'a, 'b> {
    pub hand: &'b super::assets::HandMaps<'a>,
    pub sin: f32,
    pub cos: f32,
    pub scale: f32,
    pub height: f32,
}

/// Whether the ray from a receiver toward the light tap reaches past the
/// blocker's slab: intersect the ray with that plane and test the hand
/// silhouette there.
pub(crate) fn ray_hits_hand(blocker: &Blocker<'_, '_>, recv: &Point, light: &Point) -> bool {
    if blocker.height <= recv.z || light.z <= blocker.height {
        return false;
    }
    let t = (blocker.height - recv.z) / (light.z - recv.z);
    let qx = recv.x + t * (light.x - recv.x);
    let qy = recv.y + t * (light.y - recv.y);
    let (fx, fy) = to_sprite_with(
        blocker.hand.pivot_x,
        blocker.hand.pivot_y,
        blocker.sin,
        blocker.cos,
        blocker.scale,
        qx,
        qy,
    );
    let w = blocker.hand.width as i32;
    let h = blocker.hand.height as i32;
    bilinear(blocker.hand.alpha, w, h, fx, fy) > 0.5
}

/// Frame/row-invariant math shared by every pixel of a row (or frame):
/// each hand's rotation sin/cos, each hand's sprite scale, and the
/// golden-angle light-tap offsets. Every cached value uses the exact same
/// expression as the per-pixel path it replaces (`libm::sinf`/`cosf` of
/// each hand angle; [`light_tap`] fills the offsets in tap order), so
/// routing pixel math through this context only hoists identical work.
///
/// Footprint: `MAX_SAMPLES` tap pairs plus six floats and a count — about
/// 220 bytes of caller-owned stack (24 taps × 8 B + 6 × 4 B + 1 B, padded).
/// No statics, no heap, no borrows: the context owns plain `f32`s, so it
/// can live beside the borrowed scene/hands without self-reference.
#[derive(Clone, Copy)]
pub(crate) struct Prepared {
    pub hour_sin: f32,
    pub hour_cos: f32,
    pub min_sin: f32,
    pub min_cos: f32,
    pub hour_scale: f32,
    pub minute_scale: f32,
    pub taps: [(f32, f32); MAX_SAMPLES as usize],
    pub taps_n: u8,
}

impl Prepared {
    /// Snapshot the frame invariants. Takes the already-clamped scene the
    /// row renderers hold, so clamping still happens exactly once per row.
    pub fn for_frame(scene: &ClockScene, hands: &Hands<'_>) -> Self {
        let (hour_sin, hour_cos) = (libm::sinf(scene.hour_angle), libm::cosf(scene.hour_angle));
        let (min_sin, min_cos) = (
            libm::sinf(scene.minute_angle),
            libm::cosf(scene.minute_angle),
        );
        let hour_scale = hand_scale(hands.hour.pivot_y, scene.hour_len);
        let minute_scale = hand_scale(hands.minute.pivot_y, scene.minute_len);
        let taps_n = scene.samples.clamp(1, MAX_SAMPLES);
        let mut taps = [(0.0f32, 0.0f32); MAX_SAMPLES as usize];
        for (i, slot) in taps.iter_mut().enumerate().take(usize::from(taps_n)) {
            *slot = light_tap(i as u8, taps_n, scene.light_size);
        }
        Self {
            hour_sin,
            hour_cos,
            min_sin,
            min_cos,
            hour_scale,
            minute_scale,
            taps,
            taps_n,
        }
    }
}

/// One lit surface point: linear albedo, unit normal, data specular, and
/// its dial-space position.
struct SurfacePoint {
    albedo: (f32, f32, f32),
    normal: (f32, f32, f32),
    spec_map: f32,
    pos: Point,
}

/// Fraction of the area light visible from a receiver: per tap, the union
/// of both blockers; averaged after, never multiplied as opacities.
/// Rotations, scales, and tap offsets come from [`Prepared`]; the tap loop
/// visits the same taps in the same order with the same union test — the
/// receiver position and the per-tap light position stay per-pixel, so a
/// moving light still shades dynamically.
pub(crate) fn visibility(
    scene: &ClockScene,
    hands: &Hands<'_>,
    prep: &Prepared,
    px: f32,
    py: f32,
    pz: f32,
) -> f32 {
    let hour = Blocker {
        hand: &hands.hour,
        sin: prep.hour_sin,
        cos: prep.hour_cos,
        scale: prep.hour_scale,
        height: scene.hour_h,
    };
    let minute = Blocker {
        hand: &hands.minute,
        sin: prep.min_sin,
        cos: prep.min_cos,
        scale: prep.minute_scale,
        height: scene.minute_h,
    };
    let recv = Point {
        x: px,
        y: py,
        z: pz,
    };
    let n = prep.taps_n;
    let mut open = 0u32;
    for i in 0..n {
        let (ox, oy) = prep.taps[usize::from(i)];
        let light = Point {
            x: scene.light_x + ox,
            y: scene.light_y + oy,
            z: scene.light_h,
        };
        if !(ray_hits_hand(&hour, &recv, &light) || ray_hits_hand(&minute, &recv, &light)) {
            open += 1;
        }
    }
    open as f32 / f32::from(n)
}

/// Direct light for one top-surface point: diffuse albedo plus independent
/// additive Blinn-Phong specular off a fixed straight-on view, both scaled
/// by shadow visibility, plus the unshadowed ambient floor.
fn shade(scene: &ClockScene, surf: &SurfacePoint, vis: f32) -> (f32, f32, f32) {
    let (dx, dy, dz) = (
        scene.light_x - surf.pos.x,
        scene.light_y - surf.pos.y,
        scene.light_h - surf.pos.z,
    );
    let dist = libm::sqrtf(dx * dx + dy * dy + dz * dz).max(1e-6);
    let (lx, ly, lz) = (dx / dist, dy / dist, dz / dist);
    let (nx, ny, nz) = surf.normal;
    let ndotl = (nx * lx + ny * ly + nz * lz).max(0.0);
    let diff = ndotl * vis + scene.ambient;
    // Dial fast path: dial paper/ticks ship spec_map == 0, so skip the H
    // normalize and powf with a bit-exact diffuse-only result.
    if surf.spec_map == 0.0 {
        return (
            surf.albedo.0 * diff + 0.0,
            surf.albedo.1 * diff + 0.0,
            surf.albedo.2 * diff + 0.0,
        );
    }
    // Fixed straight-on view: H = normalize(L + V) with V = (0, 0, 1).
    let (hx, hy, hz) = (lx, ly, lz + 1.0);
    let hlen = libm::sqrtf(hx * hx + hy * hy + hz * hz).max(1e-6);
    let ndoth = (nx * hx / hlen + ny * hy / hlen + nz * hz / hlen).max(0.0);
    let spec = surf.spec_map * scene.spec_strength * libm::powf(ndoth, scene.shininess) * vis;
    (
        surf.albedo.0 * diff + spec,
        surf.albedo.1 * diff + spec,
        surf.albedo.2 * diff + spec,
    )
}

/// Sprite pixels per dial unit for a hand of tip length `len` whose tip
/// sits `pivot_y` sprite pixels above its pivot (tip assumed at row 0;
/// both sprites start within a few empty rows of the top edge).
fn hand_scale(pivot_y: f32, len: f32) -> f32 {
    pivot_y / len.max(1e-6)
}

/// One composed pixel plus the facts the regional path classifies on: the
/// dial-furniture class, each painted layer's coverage in paint order
/// (lower first), and the shadow visibility at the receiver the viewer
/// actually sees. Built in the same single pass as the color, so the gray
/// derived from it can never drift from [`render_gray_row`].
#[derive(Clone, Copy)]
pub(crate) struct Composite {
    pub rgb: (f32, f32, f32),
    pub dial_clock: bool,
    pub lower_alpha: f32,
    pub upper_alpha: f32,
    pub receiver_vis: f32,
}

/// Linear RGB to the sRGB gray byte [`render_gray_row`] writes. One formula
/// in one place: both row renderers call this.
pub(crate) fn gray_byte(rgb: (f32, f32, f32)) -> u8 {
    (to_srgb(0.2126 * rgb.0 + 0.7152 * rgb.1 + 0.0722 * rgb.2) * 255.0).clamp(0.0, 255.0) as u8
}

fn pixel_linear(
    scene: &ClockScene,
    hands: &Hands<'_>,
    ok: bool,
    x: u32,
    y: u32,
) -> (f32, f32, f32) {
    pixel_composite(scene, hands, ok, x, y).rgb
}

/// The unoccluded dial behind everything: same paper/tick/hub preparation
/// and same light/shading as the full composite, but no hands and no
/// blockers (`vis = 1.0`). Shared by the composition experiment's
/// stationary base so a host can never clone the dial math and drift.
pub(crate) struct DialBase {
    pub rgb: (f32, f32, f32),
    pub dial_clock: bool,
}

/// One shared dial authority for base and full: a valid baked image
/// suppresses every procedural mark (`dial_clock` false, flat +Z normal,
/// zero specular) and samples sRGB gray through the normal pipeline;
/// otherwise the procedural paper/ticks/hub own the pixel.
fn dial_surface(scene: &ClockScene<'_>, x: u32, y: u32, px: f32, py: f32) -> (f32, f32, bool) {
    if let Some(gray) = super::dial::dial_image_sample(scene, x, y) {
        return (gray, 0.0, false);
    }
    let (albedo, spec) = dial_finish(px, py);
    // `dial_finish` returns its furniture constants verbatim, so exact
    // equality is the furniture test — no epsilon, no second opinion.
    (albedo, spec, albedo == TICK || albedo == HUB)
}

pub(crate) fn dial_base_composite(scene: &ClockScene<'_>, x: u32, y: u32) -> DialBase {
    let r = scene.radius.max(1e-6);
    let px = (x as f32 - scene.center_x) / r;
    let py = (y as f32 - scene.center_y) / r;
    let (dalbedo_srgb, dspec, dial_clock) = dial_surface(scene, x, y, px, py);
    let dalbedo = to_linear(dalbedo_srgb);
    let rgb = shade(
        scene,
        &SurfacePoint {
            albedo: (dalbedo, dalbedo, dalbedo),
            normal: (0.0, 0.0, 1.0),
            spec_map: dspec,
            pos: Point {
                x: px,
                y: py,
                z: 0.0,
            },
        },
        1.0,
    );
    DialBase { rgb, dial_clock }
}

/// The full single-pass composite [`pixel_linear`] shades, with region
/// metadata captured alongside. Shading math is untouched: this is the old
/// body with its intermediate dial class, layer alphas, and per-layer
/// visibility kept instead of dropped.
pub(crate) fn pixel_composite(
    scene: &ClockScene,
    hands: &Hands<'_>,
    ok: bool,
    x: u32,
    y: u32,
) -> Composite {
    // Single-pixel convenience: snapshot the frame invariants for exactly
    // one pixel. Row renderers hoist this out of the loop instead.
    let prep = Prepared::for_frame(scene, hands);
    pixel_composite_prepared(scene, hands, ok, &prep, x, y)
}

/// The full single-pass composite with a caller-provided [`Prepared`]
/// context. Shading math is untouched: this is the old body with its
/// intermediate dial class, layer alphas, and per-layer visibility kept
/// instead of dropped.
pub(crate) fn pixel_composite_prepared(
    scene: &ClockScene,
    hands: &Hands<'_>,
    ok: bool,
    prep: &Prepared,
    x: u32,
    y: u32,
) -> Composite {
    pixel_composite_impl(scene, hands, ok, prep, x, y, None)
}

/// The same composite with precomputed shadow visibility: `counts` holds
/// the open-tap counts for the dial, lower-hand, and upper-hand receiver
/// planes (see [`crate::shadow_masks`]), each out of `prep.taps_n`. The
/// ray loop is the only thing replaced — every shade, coverage, and
/// receiver rule below is shared with [`pixel_composite_prepared`], so the
/// two spellings agree whenever the counts match the traced taps.
pub(crate) fn pixel_composite_with_counts(
    scene: &ClockScene,
    hands: &Hands<'_>,
    ok: bool,
    prep: &Prepared,
    x: u32,
    y: u32,
    counts: (u8, u8, u8),
) -> Composite {
    pixel_composite_impl(scene, hands, ok, prep, x, y, Some(counts))
}

/// Shared body for [`pixel_composite_prepared`] (`counts = None`, trace
/// every ray) and [`pixel_composite_with_counts`] (`Some`, divide the
/// caller-supplied open counts by the tap count). `counts.1`/`.2` index
/// the painted layers lower-first, matching the `alphas`/`visibilities`
/// slots below.
fn pixel_composite_impl(
    scene: &ClockScene,
    hands: &Hands<'_>,
    ok: bool,
    prep: &Prepared,
    x: u32,
    y: u32,
    counts: Option<(u8, u8, u8)>,
) -> Composite {
    let r = scene.radius.max(1e-6);
    let px = (x as f32 - scene.center_x) / r;
    let py = (y as f32 - scene.center_y) / r;
    // One authoritative pivot per hand: the borrowed maps own both the
    // sprite offset (`to_sprite_with`) and the scale in `prep`, so an
    // override can never scale from one center while drawing from another.

    // Dial base, shadowed by whichever hands float above it.
    let (dalbedo_srgb, dspec, dial_clock) = dial_surface(scene, x, y, px, py);
    let dalbedo = to_linear(dalbedo_srgb);
    let taps_f = f32::from(prep.taps_n);
    let dial_vis = if ok {
        match counts {
            Some((dial_open, _, _)) => f32::from(dial_open) / taps_f,
            None => visibility(scene, hands, prep, px, py, 0.0),
        }
    } else {
        1.0
    };
    let mut rgb = shade(
        scene,
        &SurfacePoint {
            albedo: (dalbedo, dalbedo, dalbedo),
            normal: (0.0, 0.0, 1.0),
            spec_map: dspec,
            pos: Point {
                x: px,
                y: py,
                z: 0.0,
            },
        },
        dial_vis,
    );

    if !ok {
        return Composite {
            rgb,
            dial_clock,
            lower_alpha: 0.0,
            upper_alpha: 0.0,
            receiver_vis: 1.0,
        };
    }
    // Paint the lower hand first, including when the preview reverses heights.
    let hour = (
        &hands.hour,
        prep.hour_sin,
        prep.hour_cos,
        prep.hour_scale,
        scene.hour_h,
        scene.hour_spec,
    );
    let minute = (
        &hands.minute,
        prep.min_sin,
        prep.min_cos,
        prep.minute_scale,
        scene.minute_h,
        scene.minute_spec,
    );
    let layers = if scene.hour_h > scene.minute_h {
        [minute, hour]
    } else {
        [hour, minute]
    };
    // Coverage and visibility per painted layer, lower first: the regional
    // path reuses these instead of re-tracing shadow rays.
    let mut alphas = [0.0f32; 2];
    let mut visibilities = [1.0f32; 2];
    for (slot, (hand, sin, cos, scale, height, spec_gain)) in layers.into_iter().enumerate() {
        let sample = sample_hand_with(hand, sin, cos, scale, px, py);
        if sample.alpha <= 0.0 {
            continue;
        }
        let vis = match counts {
            // Painted-layer slots are lower-first, so slot 0 reads the
            // lower count and slot 1 the upper count.
            Some((_, lower_open, upper_open)) => {
                f32::from(if slot == 0 { lower_open } else { upper_open }) / taps_f
            }
            None => visibility(scene, hands, prep, px, py, height),
        };
        alphas[slot] = sample.alpha.clamp(0.0, 1.0);
        visibilities[slot] = vis;
        let lit = shade(
            scene,
            &SurfacePoint {
                albedo: sample.albedo,
                normal: sample.normal,
                spec_map: sample.spec * spec_gain,
                pos: Point {
                    x: px,
                    y: py,
                    z: height,
                },
            },
            vis,
        );
        // Hand contrast only: attenuate the hand's own linear shade
        // (diffuse variation and specular together) before compositing.
        // Alpha, dial, shadow geometry, and region masks are untouched.
        let keep = 1.0 - scene.hand_darkness.clamp(0.0, 1.0);
        let lit = (lit.0 * keep, lit.1 * keep, lit.2 * keep);
        let a = sample.alpha.clamp(0.0, 1.0);
        rgb = (
            lit.0 * a + rgb.0 * (1.0 - a),
            lit.1 * a + rgb.1 * (1.0 - a),
            lit.2 * a + rgb.2 * (1.0 - a),
        );
    }
    // The visible receiver is the topmost covering layer, else the lower
    // one, else the dial: its visibility is the shadow strength the region
    // classifier weights. A covering top hand is unshadowed by construction
    // (nothing floats above it), so its own pixels never flip to Shadows —
    // and a hidden lower hand's shadow can never surface through it.
    let receiver_vis = if alphas[1] >= 0.5 {
        visibilities[1]
    } else if alphas[0] >= 0.5 {
        visibilities[0]
    } else {
        dial_vis
    };
    Composite {
        rgb,
        dial_clock,
        lower_alpha: alphas[0],
        upper_alpha: alphas[1],
        receiver_vis,
    }
}

/// One composed pixel as sRGB grayscale, 0..1. Invalid hand buffers render
/// the dial alone rather than failing: geometry is checked once per call
/// chain, sampling still clips per texel.
pub fn evaluate_pixel(scene: &ClockScene, hands: &Hands<'_>, x: u32, y: u32) -> f32 {
    let scene = scene.clamped();
    let (r, g, b) = pixel_linear(&scene, hands, hands.validate(), x, y);
    to_srgb(0.2126 * r + 0.7152 * g + 0.0722 * b)
}

/// One grayscale row as sRGB bytes. Writes `min(out.len(), width)` pixels;
/// a `y` past the scene height is a no-op. Constant scratch only.
pub fn render_gray_row(scene: &ClockScene, hands: &Hands<'_>, y: u32, out: &mut [u8]) {
    let scene = scene.clamped();
    if y >= scene.height {
        return;
    }
    let ok = hands.validate();
    let prep = Prepared::for_frame(&scene, hands);
    let n = core::cmp::min(out.len(), scene.width as usize);
    for (x, slot) in out.iter_mut().take(n).enumerate() {
        *slot = gray_byte(pixel_composite_prepared(&scene, hands, ok, &prep, x as u32, y).rgb);
    }
}

/// Dithers the composed grayscale straight onto a monochrome surface: one
/// [`Dither::inks`] call per screen pixel, ink coverage = 1 - gray.
pub fn render_surface(
    scene: &ClockScene,
    hands: &Hands<'_>,
    dither: Dither,
    surface: &mut impl Surface,
) {
    let scene = scene.clamped();
    let ok = hands.validate();
    let prep = Prepared::for_frame(&scene, hands);
    for y in 0..scene.height {
        for x in 0..scene.width {
            let (r, g, b) = pixel_composite_prepared(&scene, hands, ok, &prep, x, y).rgb;
            let gray = to_srgb(0.2126 * r + 0.7152 * g + 0.0722 * b);
            surface.set(
                x as i32,
                y as i32,
                dither.inks(1.0 - gray, x as i32, y as i32),
            );
        }
    }
}

// Note on the green flip: `sample_hand` negates the filtered green
// component from stored-up into renderer y-down. `assets::decode_normal`
// is the per-texel statement of the same convention, covered by its own
// tests; the two agree by construction (same center, same negation).

#[cfg(test)]
mod tests;
