//! Shared analog-clock composition experiment: late selection between
//! independently completed candidates under one physical footprint.
//!
//! Four modes share stable host-contract names ([`CompositionMode`]):
//!
//! - `legacy`: the existing regional path, unchanged. Host calls
//!   [`crate::render_region_row`] plus [`crate::dither_regions`].
//! - `reference`: the completed full-scene gray dithered in one uniform
//!   [`DitherAlgorithm::BlueNoise`](crate::DitherAlgorithm) pass
//!   ([`crate::dither_flat`]).
//! - `replacement`: an independently completed unoccluded-dial static
//!   candidate versus the completed dynamic (full-scene regional)
//!   candidate, picked pointwise by the actual affected footprint.
//! - `removal` (experimental): the cached static candidate whose white dots
//!   thin inside the shadow footprint by a dithered keep-probability mask;
//!   hand/antialiased pixels still take the dynamic candidate.
//!
//! Row flow per frame: [`render_compose_row`] shades each pixel once and
//! emits full gray plus full regions (legacy-exact), base gray plus base
//! regions (unobstructed dial incl. ticks/hub, same light), and exact
//! physical [`Footprint`] flags. Host dithers each plane independently with
//! the current shared passes, packs the bits MSB-first (`1 = ink`, the
//! [`raster::BitCanvas`] convention), and [`compose_surface`] selects them
//! onto a [`raster::Surface`]. The core owns every bit decision; the host
//! clones no shading, threshold, or selection math.
//!
//! Footprint: caller-owned memory throughout, no statics, no allocation.
//! DRAM note: this module adds no buffers of its own — gray/region/
//! footprint planes, packed candidates, and diffusion scratch all stay
//! caller-owned, per the DRAM budget.

use super::assets::Hands;
use super::region::{classify_region, composite_cover, DitherRegion};
use super::render::{
    dial_base_composite, gray_byte, pixel_composite_prepared, pixel_composite_with_counts, Prepared,
};
use super::scene::ClockScene;
use super::shadow_masks::{MaskContext, MaskStats};
use raster::Surface;

/// Comparison mode. [`as_str`](CompositionMode::as_str) spellings are the
/// stable host contract: `legacy`, `reference`, `replacement`, `removal`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositionMode {
    Legacy,
    Reference,
    Replacement,
    Removal,
}

impl CompositionMode {
    /// Stable host-contract name. Never rename without the host owner.
    pub fn as_str(self) -> &'static str {
        match self {
            CompositionMode::Legacy => "legacy",
            CompositionMode::Reference => "reference",
            CompositionMode::Replacement => "replacement",
            CompositionMode::Removal => "removal",
        }
    }

    /// Exact-name parse; anything else is `None`. Kept next to the
    /// [`core::str::FromStr`] impl as the infallible-query spelling.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        s.parse().ok()
    }
}

/// Rejected [`CompositionMode`] name. Only the four stable names parse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownCompositionMode;

impl core::fmt::Display for UnknownCompositionMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "unknown composition mode (legacy|reference|replacement|removal)"
        )
    }
}

impl core::str::FromStr for CompositionMode {
    type Err = UnknownCompositionMode;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "legacy" => Ok(CompositionMode::Legacy),
            "reference" => Ok(CompositionMode::Reference),
            "replacement" => Ok(CompositionMode::Replacement),
            "removal" => Ok(CompositionMode::Removal),
            _ => Err(UnknownCompositionMode),
        }
    }
}

/// Exact physical footprint of moving content at one pixel: real coverage
/// and real shadow visibility, never the stochastic semantic label and
/// never a gray-byte delta. A pixel the compositor barely touches — one
/// antialiased texel, one faint penumbra tap — still flags, even when the
/// rounded gray byte is unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Footprint(u8);

impl Footprint {
    pub const NONE: Self = Self(0);
    /// Any real hand coverage (`cover > 0` on either painted layer).
    pub const HAND: Self = Self(1);
    /// No hand coverage, but the visible receiver's light is measurably
    /// blocked (`visibility < 1`, physical and pre-binary).
    pub const SHADOW: Self = Self(2);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn hand(self) -> bool {
        self.contains(Self::HAND)
    }

    pub fn shadow(self) -> bool {
        self.contains(Self::SHADOW)
    }
}

/// The physical flag for one composite. HAND wins over SHADOW: a pixel a
/// physical hand covers takes the completed-scene candidate, never the
/// dial's shadow ratio.
pub fn physical_footprint(cover: f32, receiver_vis: f32) -> Footprint {
    if cover > 0.0 {
        Footprint::HAND
    } else if receiver_vis < 1.0 {
        Footprint::SHADOW
    } else {
        Footprint::NONE
    }
}

/// One stationary (unoccluded dial) row: same paper/tick/hub preparation
/// and same light/shading as the full composite, with no hands and no
/// blockers. Takes no sprites, so there is no invalid-sprite sentinel to
/// trip over. Writes `min(gray.len(), regions.len(), width)` pixels; a `y`
/// past the scene height is a no-op. Constant scratch only.
pub fn render_base_row(scene: &ClockScene, y: u32, gray: &mut [u8], regions: &mut [DitherRegion]) {
    let scene = scene.clamped();
    if y >= scene.height {
        return;
    }
    let n = gray.len().min(regions.len()).min(scene.width as usize);
    for x in 0..n {
        let b = dial_base_composite(&scene, x as u32, y);
        gray[x] = gray_byte(b.rgb);
        regions[x] = if b.dial_clock {
            DitherRegion::Clock
        } else {
            DitherRegion::Background
        };
    }
}

/// One dynamic pixel shared by both row APIs: the full composite run
/// through the single per-pixel formula authority
/// ([`pixel_composite_prepared`], then [`gray_byte`], the shared region
/// classifier, and [`physical_footprint`]). Returns the gray byte, the
/// region label, and the footprint flag in that order.
fn dynamic_pixel(
    scene: &ClockScene,
    hands: &Hands<'_>,
    ok: bool,
    prep: &Prepared,
    x: u32,
    y: u32,
) -> (u8, DitherRegion, Footprint) {
    let c = pixel_composite_prepared(scene, hands, ok, prep, x, y);
    let gray = gray_byte(c.rgb);
    let cover = composite_cover(c.lower_alpha, c.upper_alpha);
    let region = classify_region(cover, c.dial_clock, c.receiver_vis, x, y);
    let footprint = physical_footprint(cover, c.receiver_vis);
    (gray, region, footprint)
}

/// One dynamic (full-scene) row for the frame pass: full gray plus full
/// regions (identical to [`crate::render_region_row`] for the same inputs —
/// same composite, same shared classifier, so legacy can never drift) and
/// exact [`Footprint`] flags, with no stationary base recomputation. Same
/// lighting and height-ordered union blockers as [`render_compose_row`];
/// the current stochastic region selector is untouched. Writes the shared
/// prefix of all three slices and the frame width; a `y` past the scene
/// height is a no-op. Constant scratch only.
pub fn render_dynamic_row(
    scene: &ClockScene,
    hands: &Hands<'_>,
    y: u32,
    full_gray: &mut [u8],
    full_regions: &mut [DitherRegion],
    footprint: &mut [Footprint],
) {
    let scene = scene.clamped();
    if y >= scene.height {
        return;
    }
    let ok = hands.validate();
    let prep = Prepared::for_frame(&scene, hands);
    let n = full_gray
        .len()
        .min(full_regions.len())
        .min(footprint.len())
        .min(scene.width as usize);
    for x in 0..n {
        let (gray, region, flag) = dynamic_pixel(&scene, hands, ok, &prep, x as u32, y);
        full_gray[x] = gray;
        full_regions[x] = region;
        footprint[x] = flag;
    }
}

/// One composed row for the experiment: full gray plus full regions
/// (identical to [`crate::render_region_row`] for the same inputs —
/// same composite, same shared classifier, so legacy can never drift),
/// base gray plus base regions (identical to [`render_base_row`]), and
/// exact [`Footprint`] flags. Same lighting and height-ordered union
/// blockers throughout; the current stochastic region selector is
/// untouched. Writes the shared prefix of all five slices and the frame
/// width; a `y` past the scene height is a no-op. Constant scratch only.
#[allow(clippy::too_many_arguments)]
pub fn render_compose_row(
    scene: &ClockScene,
    hands: &Hands<'_>,
    y: u32,
    full_gray: &mut [u8],
    full_regions: &mut [DitherRegion],
    base_gray: &mut [u8],
    base_regions: &mut [DitherRegion],
    footprint: &mut [Footprint],
) {
    let scene = scene.clamped();
    if y >= scene.height {
        return;
    }
    let ok = hands.validate();
    let prep = Prepared::for_frame(&scene, hands);
    let n = full_gray
        .len()
        .min(full_regions.len())
        .min(base_gray.len())
        .min(base_regions.len())
        .min(footprint.len())
        .min(scene.width as usize);
    for x in 0..n {
        let (gray, region, flag) = dynamic_pixel(&scene, hands, ok, &prep, x as u32, y);
        full_gray[x] = gray;
        full_regions[x] = region;
        footprint[x] = flag;
        let b = dial_base_composite(&scene, x as u32, y);
        base_gray[x] = gray_byte(b.rgb);
        base_regions[x] = if b.dial_clock {
            DitherRegion::Clock
        } else {
            DitherRegion::Background
        };
    }
}

/// Mutable row outputs shared by the mask composer.
pub struct ComposeRowBuffers<'a> {
    pub full_gray: &'a mut [u8],
    pub full_regions: &'a mut [DitherRegion],
    pub base_gray: &'a mut [u8],
    pub base_regions: &'a mut [DitherRegion],
    pub footprint: &'a mut [Footprint],
}

/// Caller-owned visibility counts for dial, lower hand, and upper hand.
pub struct MaskRowBuffers<'a> {
    pub dial_open: &'a mut [u8],
    pub lower_open: &'a mut [u8],
    pub upper_open: &'a mut [u8],
}

/// Retained stationary row and mutable dynamic outputs. The stationary
/// row must come from [`render_base_row`] with identical scene lighting.
pub struct CachedRowBuffers<'a> {
    pub base_gray: &'a [u8],
    pub base_regions: &'a [DitherRegion],
    pub full_gray: &'a mut [u8],
    pub full_regions: &'a mut [DitherRegion],
    pub footprint: &'a mut [Footprint],
}

/// Compose a row using shared light-sample masks, producing the stationary
/// row as well. Context and scene must describe the same frame and hands.
/// Writes the shared prefix of all buffers, leaving tails unchanged.
pub fn render_compose_row_with_masks(
    scene: &ClockScene,
    hands: &Hands<'_>,
    masks: &MaskContext,
    y: u32,
    out: ComposeRowBuffers<'_>,
    scratch: MaskRowBuffers<'_>,
) -> MaskStats {
    let scene = scene.clamped();
    let n = out
        .full_gray
        .len()
        .min(out.full_regions.len())
        .min(out.base_gray.len())
        .min(out.base_regions.len())
        .min(out.footprint.len())
        .min(scratch.dial_open.len())
        .min(scratch.lower_open.len())
        .min(scratch.upper_open.len())
        .min(scene.width as usize)
        .min(masks.width() as usize);
    if y >= scene.height || y >= masks.height() {
        return MaskStats::ZERO;
    }
    render_base_row(
        &scene,
        y,
        &mut out.base_gray[..n],
        &mut out.base_regions[..n],
    );
    render_cached_row_with_masks(
        &scene,
        hands,
        masks,
        y,
        CachedRowBuffers {
            base_gray: &out.base_gray[..n],
            base_regions: &out.base_regions[..n],
            full_gray: &mut out.full_gray[..n],
            full_regions: &mut out.full_regions[..n],
            footprint: &mut out.footprint[..n],
        },
        scratch,
    )
}

/// Restore a cached lit dial, then shade an already-prepared
/// conservative range using the counts
/// [`prepare_mask_row`](super::shadow_masks::prepare_mask_row) filled —
/// no mask generation here. Counts union both blockers for each shared
/// light sample. Dithering remains a separate full-frame pass. Context,
/// cache, counts, and scene must describe the same lighting and geometry.
/// Restores the shared prefix of all buffers (tails unchanged); a `y`
/// past the scene height is a no-op, like the legacy row. The range is
/// clamped to that prefix, so a wider prepared interval stays safe.
pub fn shade_cached_row_with_masks(
    scene: &ClockScene,
    hands: &Hands<'_>,
    masks: &MaskContext,
    y: u32,
    out: CachedRowBuffers<'_>,
    scratch: MaskRowBuffers<'_>,
    affected_range: (usize, usize),
) {
    let scene = scene.clamped();
    if y >= scene.height || y >= masks.height() {
        return;
    }
    let n = out
        .full_gray
        .len()
        .min(out.full_regions.len())
        .min(out.base_gray.len())
        .min(out.base_regions.len())
        .min(out.footprint.len())
        .min(scratch.dial_open.len())
        .min(scratch.lower_open.len())
        .min(scratch.upper_open.len())
        .min(scene.width as usize)
        .min(masks.width() as usize);
    let CachedRowBuffers {
        base_gray,
        base_regions,
        full_gray,
        full_regions,
        footprint,
    } = out;
    let MaskRowBuffers {
        dial_open,
        lower_open,
        upper_open,
    } = scratch;
    let lo = affected_range.0.min(n);
    let hi = affected_range.1.min(n);
    let (lo, hi) = if lo > hi { (hi, hi) } else { (lo, hi) };
    full_gray[..n].copy_from_slice(&base_gray[..n]);
    full_regions[..n].copy_from_slice(&base_regions[..n]);
    footprint[..n].fill(Footprint::NONE);
    let ok = hands.validate();
    for x in lo..hi {
        let c = pixel_composite_with_counts(
            &scene,
            hands,
            ok,
            masks.prepared(),
            x as u32,
            y,
            (dial_open[x], lower_open[x], upper_open[x]),
        );
        let cover = composite_cover(c.lower_alpha, c.upper_alpha);
        full_gray[x] = gray_byte(c.rgb);
        full_regions[x] = classify_region(cover, c.dial_clock, c.receiver_vis, x as u32, y);
        footprint[x] = physical_footprint(cover, c.receiver_vis);
    }
}

/// Restore a cached lit dial, then shade only conservative hand/shadow
/// bounds. Counts union both blockers for each shared light sample.
/// Dithering remains a separate full-frame pass. Context, cache and scene
/// must describe the same lighting and geometry. Scratch is caller-owned.
/// Prepares the row (counts plus the conservative range, in one pass)
/// then shades it: the returned stats match [`mask_row_counts`] exactly.
pub fn render_cached_row_with_masks(
    scene: &ClockScene,
    hands: &Hands<'_>,
    masks: &MaskContext,
    y: u32,
    out: CachedRowBuffers<'_>,
    scratch: MaskRowBuffers<'_>,
) -> MaskStats {
    let scene = scene.clamped();
    if y >= scene.height || y >= masks.height() {
        return MaskStats::ZERO;
    }
    let n = out
        .full_gray
        .len()
        .min(out.full_regions.len())
        .min(out.base_gray.len())
        .min(out.base_regions.len())
        .min(out.footprint.len())
        .min(scratch.dial_open.len())
        .min(scratch.lower_open.len())
        .min(scratch.upper_open.len())
        .min(scene.width as usize)
        .min(masks.width() as usize);
    let work = super::shadow_masks::prepare_mask_row(
        masks,
        hands,
        y,
        &mut scratch.dial_open[..n],
        &mut scratch.lower_open[..n],
        &mut scratch.upper_open[..n],
    );
    shade_cached_row_with_masks(
        &scene,
        hands,
        masks,
        y,
        CachedRowBuffers {
            base_gray: &out.base_gray[..n],
            base_regions: &out.base_regions[..n],
            full_gray: &mut out.full_gray[..n],
            full_regions: &mut out.full_regions[..n],
            footprint: &mut out.footprint[..n],
        },
        MaskRowBuffers {
            dial_open: &mut scratch.dial_open[..n],
            lower_open: &mut scratch.lower_open[..n],
            upper_open: &mut scratch.upper_open[..n],
        },
        work.affected_range,
    );
    work.stats
}

/// One removal keep-probability row from the two encoded gray rows: `keep =
/// min(1, current / base)` in quantizer tone domain (bytes over 255 — no
/// gamma, no lighting re-evaluation), scaled back to a byte. `current >=
/// base` clamps to full keep (identity: the experiment never adds white);
/// `base == 0` is guarded to zero (base black holds no white dots, so
/// black remains black whatever the mask says). Writes the shared prefix;
/// empty or short slices process what they share, like the gray rows.
pub fn removal_keep_row(base_row: &[u8], full_row: &[u8], out: &mut [u8]) {
    let n = base_row.len().min(full_row.len()).min(out.len());
    for i in 0..n {
        let b = base_row[i];
        out[i] = if b == 0 {
            0
        } else {
            let keep = (f32::from(full_row[i]) / f32::from(b)).min(1.0);
            libm::roundf(keep * 255.0) as u8
        };
    }
}

/// Fully dithered packed candidates for [`compose_surface`]: row-major,
/// MSB-first, `1 = ink` (the [`raster::BitCanvas`] convention). The host
/// produces each plane with the current shared passes — the dynamic plane
/// with [`crate::dither_regions`] over the full gray/regions, each static
/// plane with [`crate::dither_flat`] over the base gray (background
/// algorithm; the stationary clock furniture may own a second static with
/// its own algorithm, picked via the base regions) — and the removal mask
/// with [`crate::dither_flat`] over the [`removal_keep_row`] plane using
/// the shadow profile's algorithm (full uniform pass, so all seven
/// selectors stay functional even though the correlation is experimental).
#[derive(Clone, Copy, Debug)]
pub struct Candidates<'a> {
    /// Dithered unoccluded dial (background algorithm).
    pub static_bg: &'a [u8],
    /// Optional second dithered dial for clock furniture; falls back to
    /// [`Candidates::static_bg`] wherever it is `None`.
    pub static_clock: Option<&'a [u8]>,
    /// Dithered completed scene (regional pass over full gray/regions).
    pub dynamic: &'a [u8],
    /// Dithered keep-probability plane; required only for Removal.
    pub mask: Option<&'a [u8]>,
}

/// Packed bytes for a `width` x `height` candidate, or `None` when the
/// shape overflows `usize`.
pub fn packed_bits_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_add(7)
        .map(|pixels| pixels / 8)
}

/// One MSB-first ink bit (`1 = ink`); the caller guarantees the plane is
/// long enough (see [`packed_bits_len`]).
fn packed_bit(bits: &[u8], idx: usize) -> bool {
    bits[idx / 8] & (0x80 >> (idx % 8)) != 0
}

/// What can go wrong before a single composed pixel lands. All variants
/// are caller-fixable geometry or buffer sizes; the surface is never
/// partially written on error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposeError {
    /// `width` or `height` is zero: there is no frame to compose.
    EmptyFrame,
    /// `width * height` (or the packed shape) overflows `usize`.
    FrameTooLarge,
    /// The base region plane is short for `width * height`.
    RegionsTooShort { need: usize, got: usize },
    /// The footprint plane is short for `width * height`.
    FootprintTooShort { need: usize, got: usize },
    /// A packed candidate (or mask) plane is short for
    /// [`packed_bits_len`]`(width, height)`; `which` names it.
    BitsTooShort {
        which: &'static str,
        need: usize,
        got: usize,
    },
    /// Removal needs the dithered keep-probability mask.
    MissingMask,
    /// Legacy has no candidates: use [`crate::dither_regions`] directly.
    NoCandidatesForLegacy,
    /// The surface is smaller than `width` x `height`.
    SurfaceTooSmall,
}

impl core::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            ComposeError::EmptyFrame => write!(f, "zero-size frame"),
            ComposeError::FrameTooLarge => write!(f, "frame overflows usize"),
            ComposeError::RegionsTooShort { need, got } => {
                write!(f, "base region plane holds {got}, needs {need}")
            }
            ComposeError::FootprintTooShort { need, got } => {
                write!(f, "footprint plane holds {got}, needs {need}")
            }
            ComposeError::BitsTooShort { which, need, got } => {
                write!(f, "{which} plane holds {got}, needs {need}")
            }
            ComposeError::MissingMask => write!(f, "removal needs a mask candidate"),
            ComposeError::NoCandidatesForLegacy => {
                write!(f, "legacy composes through dither_regions, not candidates")
            }
            ComposeError::SurfaceTooSmall => write!(f, "surface smaller than frame"),
        }
    }
}

fn check_bits(which: &'static str, bits: &[u8], need_bytes: usize) -> Result<(), ComposeError> {
    if bits.len() < need_bytes {
        return Err(ComposeError::BitsTooShort {
            which,
            need: need_bytes,
            got: bits.len(),
        });
    }
    Ok(())
}

/// Late-select fully dithered packed candidates onto a monochrome surface:
/// one binary read per pixel, never a re-composition and never host-side
/// selection math. Every size (planes, packed bits, surface, overflow) is
/// validated up front, so a failed call writes nothing.
///
/// Per-pixel pick (`static` = the base region's static bit: clock regions
/// read `static_clock` when present, everything else reads `static_bg`):
///
/// - Reference: the dynamic bit (the host dithered the completed full
///   gray in one uniform BlueNoise pass into it).
/// - Replacement: the dynamic bit inside the affected footprint
///   (HAND or SHADOW), else the static bit — so the base carries no
///   moving holes.
/// - Removal: the dynamic bit on HAND receivers (hands/antialiasing always
///   take the completed scene, never the dial's shadow ratio), the static
///   bit OR the mask bit on SHADOW (mask ink thins static white; static
///   ink never clears, so black remains black), else the static bit.
///   Local density/correlation tolerances are measured on the host; the
///   core only applies the bits it is given.
///
/// Legacy is rejected ([`ComposeError::NoCandidatesForLegacy`]): it owns
/// no candidates and keeps composing through [`crate::dither_regions`].
#[allow(clippy::too_many_arguments)]
pub fn compose_surface(
    mode: CompositionMode,
    width: u32,
    height: u32,
    base_regions: &[DitherRegion],
    footprint: &[Footprint],
    candidates: &Candidates<'_>,
    surface: &mut impl Surface,
) -> Result<(), ComposeError> {
    if mode == CompositionMode::Legacy {
        return Err(ComposeError::NoCandidatesForLegacy);
    }
    if width == 0 || height == 0 {
        return Err(ComposeError::EmptyFrame);
    }
    let w = width as usize;
    let h = height as usize;
    let pixels = w.checked_mul(h).ok_or(ComposeError::FrameTooLarge)?;
    let need_bytes = packed_bits_len(width, height).ok_or(ComposeError::FrameTooLarge)?;
    if mode == CompositionMode::Reference {
        check_bits("dynamic", candidates.dynamic, need_bytes)?;
    } else {
        if base_regions.len() < pixels {
            return Err(ComposeError::RegionsTooShort {
                need: pixels,
                got: base_regions.len(),
            });
        }
        if footprint.len() < pixels {
            return Err(ComposeError::FootprintTooShort {
                need: pixels,
                got: footprint.len(),
            });
        }
        check_bits("static_bg", candidates.static_bg, need_bytes)?;
        if let Some(clock) = candidates.static_clock {
            check_bits("static_clock", clock, need_bytes)?;
        }
        check_bits("dynamic", candidates.dynamic, need_bytes)?;
        if mode == CompositionMode::Removal {
            match candidates.mask {
                Some(mask) => check_bits("mask", mask, need_bytes)?,
                None => return Err(ComposeError::MissingMask),
            }
        }
    }
    if i64::from(surface.width()) < i64::from(width)
        || i64::from(surface.height()) < i64::from(height)
    {
        return Err(ComposeError::SurfaceTooSmall);
    }

    for y in 0..height {
        for x in 0..width {
            let idx = y as usize * w + x as usize;
            let ink = match mode {
                CompositionMode::Legacy => {
                    debug_assert!(false, "legacy rejected above");
                    return Err(ComposeError::NoCandidatesForLegacy);
                }
                CompositionMode::Reference => packed_bit(candidates.dynamic, idx),
                CompositionMode::Replacement => {
                    if footprint[idx].is_empty() {
                        static_bit(candidates, base_regions[idx], idx)
                    } else {
                        packed_bit(candidates.dynamic, idx)
                    }
                }
                CompositionMode::Removal => {
                    let fp = footprint[idx];
                    if fp.hand() {
                        packed_bit(candidates.dynamic, idx)
                    } else if fp.shadow() {
                        // Mask ink thins static white; static ink survives:
                        // removal only ever adds ink, never paper.
                        static_bit(candidates, base_regions[idx], idx)
                            || packed_bit(expect_mask(candidates), idx)
                    } else {
                        static_bit(candidates, base_regions[idx], idx)
                    }
                }
            };
            surface.set(x as i32, y as i32, ink);
        }
    }
    Ok(())
}

/// The static bit for one base label: clock furniture reads its own
/// candidate when the host computed one, everything else (including the
/// Hands/Shadows labels, which never occur on the unoccluded dial) reads
/// the background candidate rather than inventing a third source.
fn static_bit(candidates: &Candidates<'_>, region: DitherRegion, idx: usize) -> bool {
    match region {
        DitherRegion::Clock => match candidates.static_clock {
            Some(clock) => packed_bit(clock, idx),
            None => packed_bit(candidates.static_bg, idx),
        },
        _ => packed_bit(candidates.static_bg, idx),
    }
}

/// The removal mask, present: the caller-level check above already
/// rejected `None`, so this is only a borrow split.
fn expect_mask<'a>(candidates: &Candidates<'a>) -> &'a [u8] {
    match candidates.mask {
        Some(mask) => mask,
        None => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;

    fn tiny_scene() -> ClockScene<'static> {
        ClockScene::for_size(8, 8)
    }

    #[test]
    fn mode_names_are_stable_and_roundtrip() {
        let pairs = [
            (CompositionMode::Legacy, "legacy"),
            (CompositionMode::Reference, "reference"),
            (CompositionMode::Replacement, "replacement"),
            (CompositionMode::Removal, "removal"),
        ];
        for (mode, name) in pairs {
            assert_eq!(mode.as_str(), name);
            assert_eq!(CompositionMode::from_str(name), Some(mode));
            assert_eq!(name.parse(), Ok(mode));
        }
        assert_eq!(CompositionMode::from_str("Legacy"), None);
        assert_eq!(CompositionMode::from_str(""), None);
        assert_eq!(
            "Legacy".parse::<CompositionMode>(),
            Err(UnknownCompositionMode)
        );
    }

    #[test]
    fn footprint_flags_compose_and_query() {
        assert!(Footprint::NONE.is_empty());
        assert!(Footprint::HAND.hand());
        assert!(!Footprint::HAND.shadow());
        assert!(Footprint::SHADOW.shadow());
        assert!(!Footprint::SHADOW.hand());
    }

    #[test]
    fn physical_prefers_hand_and_spots_faint_shadow() {
        assert_eq!(physical_footprint(0.0, 1.0), Footprint::NONE);
        assert_eq!(physical_footprint(0.001, 1.0), Footprint::HAND);
        assert_eq!(physical_footprint(0.0, 0.9999), Footprint::SHADOW);
        // HAND overrides SHADOW on physical hand receivers.
        assert_eq!(physical_footprint(0.5, 0.0), Footprint::HAND);
    }

    #[test]
    fn keep_row_guards_and_clamps() {
        let mut out = vec![0u8; 4];
        // c == b identity, c > b clamped identity, c < b thinned.
        removal_keep_row(&[200, 100, 0, 255], &[200, 50, 123, 255], &mut out);
        assert_eq!(out[0], 255);
        assert_eq!(out[1], 128);
        assert_eq!(out[2], 0, "base black guards the divide");
        assert_eq!(out[3], 255);
        // Brighter-than-base can never add white beyond identity.
        removal_keep_row(&[100], &[255], &mut out[..1]);
        assert_eq!(out[0], 255);
        // Half probability rounds to nearest (1/2 -> 128, not 127).
        let mut half = vec![0u8; 1];
        removal_keep_row(&[2], &[1], &mut half);
        assert_eq!(half, vec![128]);
        // Short slices share the prefix; empty is a no-op.
        let mut one = vec![9u8; 1];
        removal_keep_row(&[10, 20], &[10, 20], &mut one);
        assert_eq!(one, vec![255]);
    }

    #[test]
    fn packed_length_counts_whole_bytes() {
        assert_eq!(packed_bits_len(8, 1), Some(1));
        assert_eq!(packed_bits_len(9, 1), Some(2));
        assert_eq!(packed_bits_len(0, 8), Some(0));
        // Full-u32 frames fit `usize` on 64-bit hosts but overflow it on
        // 32-bit embedded targets, where the `None` path lives.
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            packed_bits_len(u32::MAX, u32::MAX),
            Some(2305843008139952129)
        );
        #[cfg(target_pointer_width = "32")]
        assert_eq!(packed_bits_len(u32::MAX, u32::MAX), None);
    }

    #[test]
    fn base_row_needs_no_hands_and_marks_furniture() {
        let scene = tiny_scene();
        let w = scene.width as usize;
        let mut gray = vec![0u8; w];
        let mut regions = vec![DitherRegion::Background; w];
        render_base_row(&scene, 0, &mut gray, &mut regions);
        assert!(
            regions.contains(&DitherRegion::Clock) || regions.contains(&DitherRegion::Background)
        );
        // Past-the-frame rows are a no-op, like the legacy rows.
        let mut gray = vec![7u8; w];
        let mut regions = vec![DitherRegion::Hands; w];
        render_base_row(&scene, scene.height + 2, &mut gray, &mut regions);
        assert!(gray.iter().all(|&g| g == 7));
        assert!(regions.iter().all(|&r| r == DitherRegion::Hands));
    }
}
