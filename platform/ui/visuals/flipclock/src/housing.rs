//! Static chrome around a flip card: body outline, bevel, side rails, axle
//! pins, and a drop shadow.
//!
//! None of this moves, so on the device it is drawn once when the surface is
//! entered and never touched again -- zero per-frame cost. It is also where
//! most of the three-dimensional read actually comes from. A leaf rotating in
//! empty space looks like a squash; the same leaf rotating between two rails
//! with a shadow under the assembly looks like a mechanism.

use raster::Surface;

use super::flap::Card;
use super::screen::Screen;

/// The assembly's own shadow.
///
/// Modelled as the **card's** silhouette offset in the light direction and
/// softened, rather than as a band bolted to one edge. The rails do not cast
/// it and must not set its bounds: they are frame, standing at the card's own
/// depth, while the card is the thing held proud of the back plane. That is what makes the
/// bottom and the right agree with each other: they are two visible slivers of
/// one offset rectangle, so the corner falls out correctly instead of needing
/// its own case.
///
/// The light sits a little to the left of straight down, so the offset is
/// mostly vertical with a smaller horizontal component. The penumbra is
/// deliberately asymmetric -- soft on the sides the light is leaving, nearly
/// hard on the sides facing it -- because a symmetric falloff would leak a
/// fringe onto the lit side and read as a glow rather than a shadow.
const SHADOW_DX: i32 = 3;
const SHADOW_DY: i32 = 7;
/// Penumbra depth away from the light, and towards it.
const SOFT_FAR: i32 = 8;
const SOFT_NEAR: i32 = 2;
const SHADOW_LEVEL: i32 = 5;

/// Wall thickness, and how far the walls stand clear of the card.
///
/// The gap carries two things and cannot be tightened without losing one of
/// them: it is the strip of back plane the card's own shadow falls on, and it
/// has to exceed the leaf's widest overhang -- `half_w * (camera_d /
/// (camera_d - half_h) - 1)`, ten pixels at the geometry in `mod.rs` -- or the
/// walls clip the perspective taper.
const RAIL_W: i32 = 7;
const RAIL_GAP: i32 = 11;

/// Everything behind the card: the ground it stands on, and the shadow it
/// throws onto it. Drawn before `flap::render_frame`.
pub fn draw_ground<S: Surface>(surface: &mut S, card: &Card, screen: Screen) {
    let left = card.hinge_x - card.half_w;
    let right = card.hinge_x + card.half_w;
    let top = card.hinge_y - card.half_h;
    let bottom = card.hinge_y + card.half_h;

    // Everything starts as paper; the card faces paint over it.
    for y in 0..surface.height() {
        for x in 0..surface.width() {
            surface.set(x, y, false);
        }
    }

    // Drop shadow: the card's silhouette, offset and softened.
    let (shadow_left, shadow_right) = (left + SHADOW_DX, right + SHADOW_DX);
    let (shadow_top, shadow_bottom) = (top + SHADOW_DY, bottom + SHADOW_DY);
    for y in 0..surface.height() {
        for x in 0..surface.width() {
            // Nothing to draw where the card itself stands.
            if x >= left && x <= right && y >= top && y <= bottom {
                continue;
            }
            // Distance outside the offset silhouette, stretched on the lit
            // sides so the falloff there dies within a pixel or two.
            let dx = if x < shadow_left {
                (shadow_left - x) * SOFT_FAR / SOFT_NEAR
            } else if x > shadow_right {
                x - shadow_right
            } else {
                0
            };
            let dy = if y < shadow_top {
                (shadow_top - y) * SOFT_FAR / SOFT_NEAR
            } else if y > shadow_bottom {
                y - shadow_bottom
            } else {
                0
            };
            let distance = dx.max(dy);
            if distance >= SOFT_FAR {
                continue;
            }
            let level = SHADOW_LEVEL * (SOFT_FAR - distance) / SOFT_FAR;
            if screen.ink(x, y, level as u8) {
                surface.set(x, y, true);
            }
        }
    }
}

/// The case's side walls and axle pins, drawn **after** the leaf.
///
/// Full height and in front, which is both what a desk flip clock actually
/// looks like and what stops the card appearing to move. The leaf overhangs
/// the card at every tilt, and with nothing fixed beside it the eye reads the
/// leaf's own splaying silhouette as the card's edge -- so the card seems to
/// widen and narrow as the flip runs. Walls at a fixed position give the eye
/// a stationary frame, and the overhang then reads as the leaf leaning out in
/// front of the case, which is what it is.
pub fn draw_rails<S: Surface>(surface: &mut S, card: &Card, _screen: Screen) {
    let left = card.hinge_x - card.half_w;
    let right = card.hinge_x + card.half_w;
    let (top, bottom) = (card.hinge_y - card.half_h, card.hinge_y + card.half_h);

    for y in top..=bottom {
        for offset in 0..RAIL_W {
            let inset = RAIL_GAP + offset;
            surface.set(left - inset, y, true);
            surface.set(right + inset, y, true);
        }
    }
    // Pins read as paper against the solid wall -- a highlight, not a hole.
    for dy in -2..=2i32 {
        for dx in -2..=2i32 {
            if dx * dx + dy * dy > 5 {
                continue;
            }
            let y = card.hinge_y + dy;
            surface.set(left - RAIL_GAP - RAIL_W / 2 + dx, y, false);
            surface.set(right + RAIL_GAP + RAIL_W / 2 + dx, y, false);
        }
    }
}
