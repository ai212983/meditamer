//! Perspective rasterizer for one split-flap leaf.
//!
//! # One input, everything derived
//!
//! A flip is **one continuous rotation** of a rigid leaf hinged at the card's
//! waist: it starts lying on the top half, sweeps through vertical, and ends
//! lying on the bottom half. [`Pose::at`] takes a single monotonically
//! increasing `progress` and derives every quantity a frame needs from it --
//! the angle, which half is moving, the shading, the cast shadow's depth and
//! anchor, and the visible thickness of the card's edge.
//!
//! This replaced a hand-authored table with a column per quantity, and the
//! reason is worth recording: independent columns let any row contradict the
//! others, and the contradictions are only discoverable by eye. That table
//! produced three separate bugs -- a free edge that vanished at exactly the
//! frame it should have been widest, a "landing overshoot" that projected the
//! leaf longer than the card it lands on, and a cast shadow that teleported
//! across the hinge at the crossover. None of the three is expressible here.
//! The sweep is parameterised by `cos t` directly, so:
//!
//! * `cos t` cannot exceed 1, so the leaf cannot outrun the card;
//! * `sin t` peaks at edge-on, so the free edge is automatically widest and
//!   the card's thickness most visible exactly where the face disappears;
//! * the two edge-on poses are the same pose, so nothing can differ between
//!   them;
//! * shading and shadow are continuous functions of the angle rather than
//!   per-row opinions.
//!
//! # The geometry
//!
//! Under rotation about a horizontal axis **every output row corresponds to
//! exactly one depth**, so perspective is constant along a row: the horizontal
//! sweep is a plain affine DDA and is still perspective-correct, with no
//! subdivision and no per-pixel divide. Two divides per row is the whole cost.
//!
//! With the hinge at the origin, `v` the distance from the hinge along the
//! card, `d` the camera distance and `t` the tilt:
//!
//! ```text
//! forward   s(v) = d / (d - v*sin t)          screen_x = u*s   screen_y = -v*cos t*s
//! inverse   v(k) = k*d / (d*|cos t| + k*sin t)  for the row k pixels from the hinge
//! ```
//!
//! Two consequences that decide every size in [`Card`], and which are easy to
//! clip away by accident:
//!
//! * `s >= 1` always, so the leaf is **always wider than the card** -- up to
//!   `d / (d - half_h)` at the free edge. The surface needs that overhang, and
//!   a clock face needs gutters between cards to put it in.
//! * Between 0 and roughly `2*half_h/d` radians the leaf projects **taller**
//!   than the card at rest, peaking a couple of percent up. That is correct --
//!   it is leaning towards the viewer -- but it means the surface needs
//!   headroom above the card top or the lean is silently clipped off.
//!
//! Tones go through [`Screen`], which is indexed in screen space rather than
//! in texture space. Texture-space screening scales with the leaf and moires
//! violently under resampling; screen-space screening stays locked to the panel
//! grid, reads as one consistent material, and is stable frame to frame.
//!
//! Everything is integer. `no_std` firmware here has no `libm` (see the
//! `hourglass` crate's `fixed` module doc), and parameterising by `cos t`
//! means the only transcendental needed is one integer square root.

use raster::Surface;

use super::screen::Screen;

/// Fractional bits for the scale and `v` intermediates.
const FRAC_BITS: u32 = 16;
const ONE: i64 = 1 << FRAC_BITS;

/// Fractional bits for the angle's sine and cosine.
const TRIG_BITS: u32 = 15;
const TRIG_ONE: i32 = 1 << TRIG_BITS;

/// A 1bpp bitmap, row-major, MSB first within each byte, rows byte-aligned.
/// Set bit = ink, matching the panel framebuffer's contract.
#[derive(Clone, Copy)]
pub struct Bitmap<'a> {
    pub data: &'a [u8],
    pub width: i32,
    pub height: i32,
}

impl Bitmap<'_> {
    pub const fn stride(&self) -> i32 {
        (self.width + 7) / 8
    }

    /// Out-of-range samples read as paper so callers never need edge cases.
    pub fn ink(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return false;
        }
        let index = (y * self.stride() + (x >> 3)) as usize;
        match self.data.get(index) {
            Some(byte) => byte & (0x80 >> (x & 7)) != 0,
            None => false,
        }
    }
}

/// The four card halves a flip moves between.
///
/// Taking all four and choosing internally, rather than asking the caller for
/// "the static ones and the moving one", removes the last place a caller could
/// contradict the pose: which half is in motion is a property of the angle, so
/// it is decided here.
#[derive(Clone, Copy)]
pub struct Faces<'a> {
    pub outgoing_top: Bitmap<'a>,
    pub outgoing_bottom: Bitmap<'a>,
    pub incoming_top: Bitmap<'a>,
    pub incoming_bottom: Bitmap<'a>,
}

/// Card and camera geometry, in surface pixels.
#[derive(Clone, Copy)]
pub struct Card {
    /// Hinge centre in surface coordinates.
    pub hinge_x: i32,
    pub hinge_y: i32,
    /// Half the card width; the card spans `hinge_x - half_w ..= hinge_x + half_w`.
    pub half_w: i32,
    /// Hinge to free edge. One half-card is this tall, the whole card twice.
    pub half_h: i32,
    /// Camera distance. Sets the taper: `4 * half_h` gives about 28 percent
    /// overhang at the free edge, `6 * half_h` about 17. Below `2 * half_h`
    /// the projection degenerates.
    pub camera_d: i32,
}

/// Which half is in motion. Derived from the angle, never chosen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// The outgoing digit's top half, falling from flat to edge-on.
    Falling,
    /// The incoming digit's bottom half, continuing from edge-on down to flat.
    Rising,
}

impl Phase {
    /// +1 draws downwards from the hinge, -1 upwards.
    const fn direction(self) -> i32 {
        match self {
            Self::Falling => -1,
            Self::Rising => 1,
        }
    }
}

/// Shapes how progress is spent across a flip.
///
/// Applied to `progress` before [`Pose::at`] sees it, so it changes the timing
/// and nothing else -- every geometric and tonal invariant still holds, because
/// they are all functions of the resulting pose rather than of the clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum Easing {
    /// Constant screen displacement per unit time. The sweep is already linear
    /// in `cos t`, and `d(cos t)/dt = -sin t` means displacement per *degree*
    /// is fastest near edge-on -- so this is the curve that does **not**
    /// stutter through the middle of the flip. Identity, and the default.
    #[default]
    UniformCosine,
    /// Accelerating, like a leaf released under gravity: slow off the mark,
    /// fastest as it lands. More mechanical, at the cost of a visible jump
    /// through the edge-on frames at low frame counts.
    Gravity,
    /// Symmetric ease-in-out. Slow at both ends, quick through the middle --
    /// reads as deliberate rather than dropped.
    Smooth,
}

impl Easing {
    pub const fn apply(self, progress: u32) -> u32 {
        if progress >= PROGRESS_MAX {
            return PROGRESS_MAX;
        }
        let p = progress as u64;
        let max = PROGRESS_MAX as u64;
        match self {
            Self::UniformCosine => progress,
            Self::Gravity => (p * p / max) as u32,
            // 3p^2 - 2p^3, in fixed point.
            Self::Smooth => ((3 * p * p * max - 2 * p * p * p) / (max * max)) as u32,
        }
    }
}

/// `progress` runs `0..=PROGRESS_MAX` across one complete flip. Any resolution
/// works -- a frame index, a timer, a scrubbed value -- and anything past the
/// end clamps to rest rather than wrapping into a new flip.
pub const PROGRESS_MAX: u32 = 4096;

/// Where the leaf first reaches flat. The remaining progress is the bounce.
const LANDING: u32 = PROGRESS_MAX * 7 / 8;
/// How far the leaf kicks back up after impact, in `cos t`.
const BOUNCE: i32 = TRIG_ONE / 12;

/// Peak values, all reached at edge-on and scaled by `sin t` from there.
const MAX_FACE_DITHER: i32 = 4;
const MAX_SHADOW_PX: i32 = 22;
const MAX_EDGE_PX: i32 = 6;
/// Extra silhouette width beyond the one pixel the leaf always carries.
const EXTRA_SILHOUETTE_PX: i32 = 1;

/// Darkness of the cast shadow at its anchor, in sixteenths, across the flip.
/// Scaled by progress through the rotation rather than by `sin t` -- see
/// [`Pose::shadow_level`] for why this one is different from the others.
const SHADOW_LEVEL_LIFTED: i32 = 3;
const SHADOW_LEVEL_LANDING: i32 = 15;

/// One pose of the leaf: an angle, and everything that follows from it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pose {
    /// `cos t` in Q15, +1 at rest on the top half through -1 at rest on the
    /// bottom. Never outside that range: it is the sweep parameter itself.
    cos: i32,
    /// `sin t` in Q15, never negative -- the leaf only ever leans towards the
    /// camera, and it peaks at edge-on.
    sin: i32,
}

impl Pose {
    /// Derives a pose from one monotonically increasing value.
    ///
    /// The sweep is linear in `cos t` rather than in `t`. `d(cos t)/dt =
    /// -sin t`, so displacement per degree is fastest near edge-on and
    /// sampling uniformly in the angle makes the middle of the flip stutter;
    /// linear in cosine equalises the screen displacement per step. It also
    /// means no sine table is needed -- `sin t` is one integer square root.
    pub const fn at(progress: u32) -> Self {
        let cos = if progress >= PROGRESS_MAX {
            -TRIG_ONE
        } else if progress <= LANDING {
            // +1 down to -1 across the fall.
            TRIG_ONE - (2 * TRIG_ONE as i64 * progress as i64 / LANDING as i64) as i32
        } else {
            // The bounce: a parabolic arc back up off the stop and down again.
            // Zero at both ends, so it joins the fall and the rest pose without
            // a step. This is the one non-monotone quantity in the whole
            // renderer, and it lives here rather than in any derived value.
            let span = PROGRESS_MAX - LANDING;
            let q = progress - LANDING;
            let arc = 4 * q as i64 * (span - q) as i64 / (span as i64 * span as i64);
            -TRIG_ONE + (BOUNCE as i64 * arc) as i32
        };
        // sin t = sqrt(1 - cos^2 t), non-negative over the whole sweep.
        let sin =
            isqrt((TRIG_ONE as i64 * TRIG_ONE as i64 - cos as i64 * cos as i64) as u64) as i32;
        Self { cos, sin }
    }

    pub const fn cos_q15(self) -> i32 {
        self.cos
    }

    pub const fn sin_q15(self) -> i32 {
        self.sin
    }

    pub const fn phase(self) -> Phase {
        if self.cos >= 0 {
            Phase::Falling
        } else {
            Phase::Rising
        }
    }

    const fn abs_cos(self) -> i32 {
        if self.cos < 0 {
            -self.cos
        } else {
            self.cos
        }
    }

    /// Shading on the leaf's face, in sixteenths. Zero when flat -- the leaf is
    /// then flush with the card and lit identically -- rising to a peak
    /// edge-on. Deliberately light: the face is a lit card, and it has to stay
    /// lighter than the shadow it throws or it reads as a hole.
    pub const fn face_dither(self) -> u8 {
        ((self.sin * MAX_FACE_DITHER) >> TRIG_BITS) as u8
    }

    /// Depth of the cast shadow, growing with how far the leaf stands off the
    /// card.
    pub const fn shadow_px(self) -> u8 {
        ((self.sin * MAX_SHADOW_PX) >> TRIG_BITS) as u8
    }

    /// Darkness of the cast shadow at its anchor, in sixteenths.
    ///
    /// The one derived quantity that does **not** scale with `sin t`. An
    /// occluder close to a surface throws a dark, tight shadow; one far from it
    /// throws a light, diffuse one. Across a flip the leaf is descending
    /// towards the card it lands on the whole way, so the shadow should start
    /// light and finish dark -- and `(1 - cos t)/2` is exactly progress through
    /// that rotation. Scaling this by `sin t` like the rest would make it
    /// symmetric about edge-on, which is what leaves every frame's shadow
    /// looking the same weight.
    ///
    /// It follows the bounce for free: the leaf kicking back up off the stop
    /// lightens its own shadow, because `cos t` moves back towards flat.
    pub const fn shadow_level(self) -> u8 {
        (SHADOW_LEVEL_LIFTED
            + ((SHADOW_LEVEL_LANDING - SHADOW_LEVEL_LIFTED) * self.descent()) / TRIG_ONE)
            as u8
    }

    /// Darkness at the far end of the shadow band.
    ///
    /// The band's *depth* scales with `sin t`, which is symmetric about
    /// edge-on, so on its own it makes the shadow peak mid-flip and fade at
    /// both ends -- which is what leaves the second half looking no heavier
    /// than the first however dark the anchor is. The falloff is therefore
    /// tied to the descent too: a contact shadow is hard-edged and holds its
    /// weight across the whole band, a distant one washes out within a few
    /// rows. Late in the flip the band is short but nearly uniform, which is
    /// what makes it read as darker despite covering less.
    pub const fn shadow_falloff_level(self) -> u8 {
        let near = self.shadow_level() as i32;
        let lifted = TRIG_ONE - self.descent();
        (near - (near * 3 * lifted) / (4 * TRIG_ONE)) as u8
    }

    /// Progress through the rotation, 0 at rest on the outgoing half and
    /// `TRIG_ONE` at rest on the incoming one. Monotone across the fall; the
    /// bounce walks it back slightly, which is what lets a rebounding leaf
    /// lighten its own shadow.
    const fn descent(self) -> i32 {
        (TRIG_ONE - self.cos) / 2
    }

    /// Width of the leaf's own outline, in pixels.
    ///
    /// Unlike the rest of the family this does **not** vanish at rest, and
    /// must not: at rest the leaf is flush with the card and its edges *are*
    /// the card's border, so one pixel is exactly the border being drawn by
    /// whoever is in front. Widening it to two at rest paints a second column
    /// just inside the border and the top half of the card looks bolder than
    /// the bottom. It widens only once the leaf has tilted far enough to
    /// overhang, where a single pixel would be lost in the face's own dither.
    pub const fn silhouette_px(self) -> i32 {
        1 + ((self.sin * EXTRA_SILHOUETTE_PX) >> TRIG_BITS)
    }

    /// Visible thickness of the card's edge. Invisible when flat, fullest
    /// edge-on -- which is exactly where the face has no height at all, so it
    /// is the only thing left to draw.
    pub const fn edge_px(self) -> u8 {
        ((self.sin * MAX_EDGE_PX) >> TRIG_BITS) as u8
    }
}

/// Integer square root by Newton's method. The argument is at most `2^30`.
const fn isqrt(value: u64) -> u64 {
    if value == 0 {
        return 0;
    }
    let mut estimate = value;
    let mut next = estimate.div_ceil(2);
    while next < estimate {
        estimate = next;
        next = (estimate + value / estimate) / 2;
    }
    estimate
}

/// `v(k)`: distance along the card, from the hinge, that screen row `k`
/// samples. Returned in Q16. Monotonic in `k`, which is what lets the caller
/// walk rows outward and stop at the free edge.
fn v_at_row(card: &Card, pose: Pose, k: i64) -> i64 {
    let denominator = card.camera_d as i64 * pose.abs_cos() as i64 + k * pose.sin as i64;
    if denominator <= 0 {
        return i64::MAX;
    }
    (k * card.camera_d as i64 * TRIG_ONE as i64 * ONE) / denominator
}

/// `s(v)`: the perspective magnification at depth `v`, in Q16. Always at or
/// above 1 -- the leaf leans towards the camera, never away.
fn scale_at(card: &Card, pose: Pose, v: i64) -> i64 {
    let d = card.camera_d as i64;
    let denominator = d - (v * pose.sin as i64) / TRIG_ONE as i64;
    if denominator <= 0 {
        return ONE;
    }
    (d * ONE) / denominator
}

/// Screen row and half-width of the leaf's free edge, as a row offset from the
/// hinge.
///
/// Derived analytically from `v = half_h` rather than from wherever the face
/// rasterizer stopped. The two agree at low tilt, but at edge-on the face has
/// no height at all while the free edge is at its *closest* approach to the
/// camera and therefore at its widest -- the single widest thing in the whole
/// flip. Reading it off the face loop loses that frame completely.
fn free_edge(card: &Card, pose: Pose) -> (i32, i32) {
    let half_h = card.half_h as i64;
    let scale = scale_at(card, pose, half_h);
    let row = ((half_h * pose.abs_cos() as i64 * scale) >> (FRAC_BITS + TRIG_BITS)) as i32;
    let half_width = ((card.half_w as i64 * scale) >> FRAC_BITS) as i32;
    (row, half_width)
}

/// How far the free edge projects from the hinge, for callers that need to
/// check the leaf against the card it lands on.
pub fn free_edge_row(card: &Card, pose: Pose) -> i32 {
    free_edge(card, pose).0
}

fn fill_span<S: Surface>(surface: &mut S, y: i32, x0: i32, x1: i32, level: u8, screen: Screen) {
    for x in x0..=x1 {
        surface.set(x, y, screen.ink(x, y, level));
    }
}

/// The card's left and right border, drawn **before** the leaf.
///
/// The leaf overhangs the card at every tilt, so a side border painted
/// afterwards lands in the middle of it and reads as a phantom card outline
/// struck through the moving leaf. It is the card behind that owns these
/// lines, and the leaf is in front of them.
fn draw_border_sides<S: Surface>(surface: &mut S, card: &Card) {
    let (left, right) = (card.hinge_x - card.half_w, card.hinge_x + card.half_w);
    let (top, bottom) = (card.hinge_y - card.half_h, card.hinge_y + card.half_h);
    for y in top..=bottom {
        surface.set(left, y, true);
        surface.set(right, y, true);
    }
}

/// The card's top and bottom border, drawn **after** the leaf.
///
/// Unlike the sides these cannot produce a phantom line: the leaf only reaches
/// the card's outermost row when it is flat against it, and there its own free
/// edge coincides with that border. Drawing them before the leaf instead lets
/// a resting leaf paint its face straight over them and the card loses its top
/// or bottom edge entirely.
fn draw_border_caps<S: Surface>(surface: &mut S, card: &Card) {
    let (left, right) = (card.hinge_x - card.half_w, card.hinge_x + card.half_w);
    let (top, bottom) = (card.hinge_y - card.half_h, card.hinge_y + card.half_h);
    for x in left..=right {
        surface.set(x, top, true);
        surface.set(x, bottom, true);
    }
}

/// Blits a half-card at rest, one source row per screen row.
fn draw_static_half<S: Surface>(surface: &mut S, card: &Card, half: &Bitmap, below: bool) {
    for row in 0..card.half_h {
        let y = if below {
            card.hinge_y + 1 + row
        } else {
            card.hinge_y - card.half_h + row
        };
        // Inset by one so the card's border survives underneath -- the faces
        // are drawn over it, not around it.
        for column in 1..(half.width - 1) {
            surface.set(
                card.hinge_x - card.half_w + column,
                y,
                half.ink(column, row),
            );
        }
    }
}

/// Rasterizes the moving leaf's face. Paints nothing when the leaf is edge-on
/// and the face has collapsed to no height; the free edge is the caller's, via
/// [`free_edge`].
fn draw_leaf<S: Surface>(surface: &mut S, card: &Card, pose: Pose, leaf: &Bitmap, screen: Screen) {
    let phase = pose.phase();
    let direction = phase.direction();
    // The source's row 0 sits at the hinge for a bottom half and at the free
    // edge for a top half, so `v` indexes it from opposite ends.
    let hinge_at_row0 = phase == Phase::Rising;
    // `v` is the distance from the hinge, running 1..=half_h. That is the same
    // scale `draw_static_half` places its rows on -- the row at distance `v`
    // lands on the same screen row either way -- so the two agree pixel for
    // pixel when the leaf is flat. Indexing the source from `half_h - 1`
    // instead puts every row of the moving leaf one off from the static half
    // it is meant to be continuous with, which reads as the digit sliding as
    // the flip starts, and drops the card's outermost row entirely.
    let half_h = card.half_h as i64;
    let face_dither = pose.face_dither();
    let outline = pose.silhouette_px();

    let mut k: i64 = 0;
    loop {
        k += 1;
        let v_near = v_at_row(card, pose, k);
        if v_near > half_h << FRAC_BITS {
            break;
        }
        let v_far = v_at_row(card, pose, k + 1);

        let y = card.hinge_y + direction * k as i32;
        if y < 0 || y >= surface.height() {
            break;
        }

        // Source rows this screen row covers. OR-accumulating them, rather
        // than picking the nearest, is what stops a digit's horizontal strokes
        // from dropping out as the leaf compresses -- and it makes the glyph
        // read as denser at high tilt, which is what a foreshortened card
        // catching less light actually looks like.
        // The span a screen row covers is half-open, `[v(k), v(k+1))`. Taking
        // `floor(v(k+1))` as the last row makes it closed, so at rest -- where
        // `v(k) = k` exactly -- every screen row ORs two source rows and the
        // moving leaf's glyph comes out a row thicker than the static half it
        // is covering. Backing off one fixed-point unit makes the end
        // exclusive without a second divide.
        let row_near = (v_near >> FRAC_BITS).clamp(1, half_h);
        let row_far = ((v_far - 1) >> FRAC_BITS).clamp(row_near, half_h);

        let scale = scale_at(card, pose, v_near >> FRAC_BITS);
        let half_width = ((card.half_w as i64 * scale) >> FRAC_BITS) as i32;

        for dx in -half_width..=half_width {
            let source_x = ((dx as i64 * ONE) / scale) as i32 + card.half_w;
            let mut source_ink = false;
            for row in row_near..=row_far {
                let source_y = if hinge_at_row0 {
                    (row - 1) as i32
                } else {
                    (half_h - row) as i32
                };
                if leaf.ink(source_x, source_y) {
                    source_ink = true;
                    break;
                }
            }
            let x = card.hinge_x + dx;
            // The leaf's own outline. This is what separates it from the
            // static half behind it and from its own cast shadow; without it
            // the three merge into one undifferentiated band and the depth
            // cue is gone.
            let silhouette = dx < -half_width + outline || dx > half_width - outline;
            let ink = silhouette || source_ink || screen.ink(x, y, face_dither);
            surface.set(x, y, ink);
        }
    }
}

/// Draws one composited frame: both static halves, the card border, the hinge
/// gap, the cast shadow, the moving leaf, and the leaf's edge thickness.
pub fn render_frame<S: Surface>(
    surface: &mut S,
    card: &Card,
    pose: Pose,
    faces: &Faces<'_>,
    screen: Screen,
) {
    let phase = pose.phase();
    // The static halves are the same throughout: the incoming digit's top is
    // revealed the moment the leaf leaves it, and the outgoing digit's bottom
    // stays put until the leaf comes down over it.
    draw_static_half(surface, card, &faces.incoming_top, false);
    draw_static_half(surface, card, &faces.outgoing_bottom, true);
    draw_border_sides(surface, card);

    // Cast shadow. It always falls **downwards**, from whichever edge of the
    // leaf is lowest, because the light is above the card:
    //
    // * falling, the leaf is above the hinge and hinged at it, so its lowest
    //   point is the hinge and the shadow runs down from there;
    // * rising, the leaf is below the hinge and lifted off the card, so its
    //   lowest point is the free edge and the shadow runs down from that.
    //
    // Mirroring it across the hinge instead -- "always on the far side from
    // the leaf" -- puts it above the hinge while rising, which is a light
    // source underneath the clock, and makes the two edge-on poses differ from
    // each other when they are the same leaf in the same position.
    let (edge_row, edge_half_width) = free_edge(card, pose);
    let anchor = match phase {
        // Below the seam, which is drawn on top of everything at the end.
        Phase::Falling => card.hinge_y + 1,
        Phase::Rising => card.hinge_y + edge_row,
    };
    let depth = pose.shadow_px() as i32;
    let near = pose.shadow_level() as i32;
    let far = pose.shadow_falloff_level() as i32;
    for step in 1..=depth {
        let y = anchor + step;
        // Linear falloff from the casting edge outward. A flat band reads as a
        // grey stripe painted on the card; a falling one reads as thrown light.
        let level = near - ((near - far) * (step - 1)) / depth.max(1);
        // Clipped to the card's interior. The occluder is a leaf that is wider
        // than the card at every tilt, so its shadow covers the card's whole
        // width -- offsetting the band the way the housing's own drop shadow is
        // offset would push ink past the right border and starve the left, and
        // because the band travels down the card as the leaf falls, that reads
        // as the card itself shifting about mid-animation.
        for x in (card.hinge_x - card.half_w + 1)..=(card.hinge_x + card.half_w - 1) {
            if screen.ink(x, y, level as u8) {
                surface.set(x, y, true);
            }
        }
    }

    let leaf = match phase {
        Phase::Falling => faces.outgoing_top,
        Phase::Rising => faces.incoming_bottom,
    };
    draw_leaf(surface, card, pose, &leaf, screen);

    // Card thickness, standing on the free edge and leaning out past it.
    let edge_px = pose.edge_px() as i32;
    let edge_y = card.hinge_y + phase.direction() * edge_row;
    for step in 0..edge_px {
        let y = edge_y + phase.direction() * step;
        fill_span(
            surface,
            y,
            card.hinge_x - edge_half_width,
            card.hinge_x + edge_half_width,
            16,
            screen,
        );
    }
    // A paper highlight along the lit side of the lip. One row, and it is the
    // difference between a card with thickness and a drawn line.
    if edge_px > 0 {
        let y = edge_y + phase.direction() * edge_px;
        fill_span(
            surface,
            y,
            card.hinge_x - edge_half_width + 1,
            card.hinge_x + edge_half_width - 1,
            0,
            screen,
        );
    }

    // The seam goes on last, with the caps.
    //
    // One ink row for the gap the two halves meet across, and a paper lip on
    // its lower side so they read as separate pieces rather than one printed
    // card. Drawn before the leaf instead, the lip survives the falling phase
    // -- where nothing covers it -- but is painted over during the rising one,
    // so it vanishes at the end of every flip and reappears as the next
    // begins. It belongs to the card assembly, not to either half.
    fill_span(
        surface,
        card.hinge_y,
        card.hinge_x - card.half_w + 1,
        card.hinge_x + card.half_w - 1,
        16,
        screen,
    );
    fill_span(
        surface,
        card.hinge_y + 1,
        card.hinge_x - card.half_w + 1,
        card.hinge_x + card.half_w - 1,
        0,
        screen,
    );

    draw_border_caps(surface, card);
}
