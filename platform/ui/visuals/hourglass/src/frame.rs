//! Device-neutral rasterization of one hourglass frame: glass walls and
//! rims, grains, and flow tracers, plus the line/square primitives they are
//! built from.
//!
//! Generic over `raster::Surface` (the external `platform/ui/visuals/raster` crate's
//! canvas contract, distinct from this crate's [`crate::raster`] projection
//! module) -- the same one-bit destination `enso` and `flipclock` draw into,
//! so a host preview and a firmware canvas share this one implementation.
//! This module names no LVGL type and no hardware type; a consuming target
//! adapts its own canvas onto `raster::Surface` beside the screen that owns
//! it, the same arrangement `flipclock` uses.
//!
//! This is the monochrome frame paint only. Clearing the canvas to paper,
//! canvas storage/token handling, labels, invalidation, and refresh policy
//! all stay with the product screen that calls [`render_frame`].

use ::raster::Surface;

use crate::backend::{wall_half_width_px, RENDER_HALF_SIZE_PX};
use crate::fixed::{Fx, Vec2};
use crate::particles::HALF_HEIGHT;
use crate::presentation::FrameData;
use crate::raster::CellProjection;

/// Glass-local pixel span each wall segment samples before re-projecting.
/// Balances curve fidelity against the per-frame cost of resampling the
/// active collision profile.
const WALL_SAMPLE_PX: i32 = 3;

/// Paints one frame's glass walls and rims, grains, and flow tracers onto
/// `surface`, centred on the surface's own midpoint. Does not clear the
/// surface first -- the caller decides what a blank frame looks like.
pub fn render_frame<S: Surface>(surface: &mut S, frame: &FrameData<'_>) {
    let projection = CellProjection::new(frame.angle, surface.width() / 2, surface.height() / 2);

    // Sample the active collision profile every three local pixels. The
    // production cellular backend supplies a rounded cubic Bezier bulb; the
    // optional PBD control supplies a straight taper.
    let half_height = HALF_HEIGHT.to_int();
    for side in [-1, 1] {
        let mut y0 = -half_height;
        while y0 < half_height {
            let y1 = (y0 + WALL_SAMPLE_PX).min(half_height);
            draw_wall_segment(surface, y0, y1, side, projection);
            y0 = y1;
        }
    }
    let rim_half_width = Fx::from_int(wall_half_width_px(half_height));
    let (top_left_x, top_left_y) = projection.project(Vec2::new(-rim_half_width, -HALF_HEIGHT));
    let (top_right_x, top_right_y) = projection.project(Vec2::new(rim_half_width, -HALF_HEIGHT));
    draw_line(surface, top_left_x, top_left_y, top_right_x, top_right_y);
    let (bottom_left_x, bottom_left_y) =
        projection.project(Vec2::new(-rim_half_width, HALF_HEIGHT));
    let (bottom_right_x, bottom_right_y) =
        projection.project(Vec2::new(rim_half_width, HALF_HEIGHT));
    draw_line(
        surface,
        bottom_left_x,
        bottom_left_y,
        bottom_right_x,
        bottom_right_y,
    );

    // Cellular grains rasterize their rotated unit lattice area so a dense
    // pile stays dense at non-cardinal angles. The retained PBD control keeps
    // its radius-one square rendering.
    for position in frame.positions() {
        if RENDER_HALF_SIZE_PX == 0 {
            for (x, y) in projection.cell_pixels(position) {
                surface.set(x, y, true);
            }
        } else {
            let (x, y) = projection.project(position);
            fill_square(surface, x, y, RENDER_HALF_SIZE_PX);
        }
    }

    // Sub-pixel flow tracers remain exactly one output pixel each. Overlap is
    // naturally a set union because every tracer writes the same canvas bit.
    for (index, position) in frame.tracer_positions.iter().enumerate() {
        if frame.tracer_active_mask & (1 << index) != 0 {
            let (x, y) = projection.project(*position);
            surface.set(x, y, true);
        }
    }
}

/// Bresenham, integer-only: every boundary segment and grain outline in this
/// widget is drawn through this one primitive.
fn draw_line<S: Surface>(surface: &mut S, x0: i32, y0: i32, x1: i32, y1: i32) {
    let (mut x, mut y) = (x0, y0);
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        surface.set(x, y, true);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

fn fill_square<S: Surface>(surface: &mut S, center_x: i32, center_y: i32, half: i32) {
    for y in (center_y - half)..=(center_y + half) {
        for x in (center_x - half)..=(center_x + half) {
            surface.set(x, y, true);
        }
    }
}

fn draw_wall_segment<S: Surface>(
    surface: &mut S,
    y0: i32,
    y1: i32,
    side: i32,
    projection: CellProjection,
) {
    let p0 = Vec2::new(
        Fx::from_int(side * wall_half_width_px(y0)),
        Fx::from_int(y0),
    );
    let p1 = Vec2::new(
        Fx::from_int(side * wall_half_width_px(y1)),
        Fx::from_int(y1),
    );
    let (x0, y0) = projection.project(p0);
    let (x1, y1) = projection.project(p1);
    draw_line(surface, x0, y0, x1, y1);
}

#[cfg(test)]
mod tests {
    use ::raster::BitCanvas;

    use super::*;
    use crate::model::HourglassModel;

    const WIDTH: i32 = 140;
    const HEIGHT: i32 = 200;
    const CANVAS_BYTES: usize = BitCanvas::bytes_for(WIDTH, HEIGHT);

    fn render(model: &HourglassModel) -> [u8; CANVAS_BYTES] {
        let mut bits = [0u8; CANVAS_BYTES];
        let mut canvas = BitCanvas::new(WIDTH, HEIGHT, &mut bits).expect("canvas");
        render_frame(&mut canvas, &FrameData::capture(model));
        bits
    }

    /// FNV-1a over the whole packed canvas -- a byte-for-byte fingerprint, not
    /// just a count, so a hole or a shifted line changes it even when the
    /// total ink coverage happens to come out the same.
    fn fingerprint(bits: &[u8]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for &byte in bits {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    /// Pins the exact pixels of a fresh, unstarted hourglass -- glass, rims,
    /// and the stationary upper pile, with no active flow tracers. Any change
    /// to the wall sampling, grain projection, rim endpoints, or primitives
    /// changes this.
    #[test]
    fn fresh_frame_matches_golden_fingerprint() {
        let model = HourglassModel::new();
        #[cfg(not(feature = "hourglass-pbd"))]
        let expected = 0x2c8c_6b4c_c041_daa5;
        #[cfg(feature = "hourglass-pbd")]
        let expected = 0xb137_d816_5e8f_8379;
        assert_eq!(fingerprint(&render(&model)), expected);
    }

    /// Ten seconds in, upright: grains have fallen and the golden bulb/throat
    /// collision profile is exercised, not just the rim's straight top and
    /// bottom.
    #[test]
    fn upright_progress_frame_matches_golden_fingerprint() {
        let mut model = HourglassModel::new();
        assert!(model.start());
        for _ in 0..crate::model::PHYSICS_HZ * 10 {
            model.tick();
        }
        #[cfg(not(feature = "hourglass-pbd"))]
        let expected = 0x563f_e5fa_b47c_0d52;
        #[cfg(feature = "hourglass-pbd")]
        let expected = 0x2e8c_5617_398b_b895;
        assert_eq!(fingerprint(&render(&model)), expected);
    }

    /// Rendering is a pure function of the captured frame: two renders of the
    /// same snapshot must agree bit for bit.
    #[test]
    fn rendering_the_same_frame_twice_is_deterministic() {
        let mut model = HourglassModel::new();
        assert!(model.start());
        for _ in 0..37 {
            model.tick();
        }
        let frame = FrameData::capture(&model);
        let mut first = [0u8; CANVAS_BYTES];
        let mut second = [0u8; CANVAS_BYTES];
        render_frame(
            &mut BitCanvas::new(WIDTH, HEIGHT, &mut first).expect("canvas"),
            &frame,
        );
        render_frame(
            &mut BitCanvas::new(WIDTH, HEIGHT, &mut second).expect("canvas"),
            &frame,
        );
        assert_eq!(first, second);
    }

    /// A fresh frame has ink: this catches a renderer that silently draws
    /// nothing (an empty canvas would trivially "match" a bad fingerprint
    /// change made in step with it).
    #[test]
    fn fresh_frame_is_not_blank() {
        let model = HourglassModel::new();
        let mut bits = render(&model);
        let canvas = BitCanvas::new(WIDTH, HEIGHT, &mut bits).expect("canvas");
        assert!(canvas.ink_count() > 0);
    }

    /// The renderer must clip to whatever surface it is given rather than
    /// assume the product's 140x200 canvas -- `raster::Surface`'s contract,
    /// and the property that lets one implementation serve any target.
    #[test]
    fn render_frame_clips_to_a_smaller_surface_without_panicking() {
        const SMALL_W: i32 = 40;
        const SMALL_H: i32 = 60;
        let mut bits = [0u8; BitCanvas::bytes_for(SMALL_W, SMALL_H)];
        let mut canvas = BitCanvas::new(SMALL_W, SMALL_H, &mut bits).expect("canvas");
        let model = HourglassModel::new();
        render_frame(&mut canvas, &FrameData::capture(&model));
        assert!(canvas.ink_count() > 0);
    }
}
