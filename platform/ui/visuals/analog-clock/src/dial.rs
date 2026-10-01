//! Dial furniture: paper, minute/hour ticks, and the central hub.
//!
//! Flat (normal +Z, low specular) and drawn in sRGB gray; the renderer
//! shades it like any other receiver, so hand shadows fall across it.

/// sRGB gray of the paper field.
pub const PAPER: f32 = 0.92;
/// sRGB gray of the tick marks.
pub const TICK: f32 = 0.18;
/// sRGB gray of the central hub cap.
pub const HUB: f32 = 0.30;
/// Hub radius, dial units.
pub const HUB_RADIUS: f32 = 0.055;
/// Diffuse specular strength of the flat dial (the hands carry the gloss).
pub const DIAL_SPEC: f32 = 0.15;

/// Bilinear read of a tightly packed gray plane with clamped edge texels.
fn bilinear_gray(map: &[u8], w: i32, h: i32, fx: f32, fy: f32) -> f32 {
    let fx = fx.clamp(0.0, (w - 1) as f32);
    let fy = fy.clamp(0.0, (h - 1) as f32);
    let x0 = libm::floorf(fx) as i32;
    let y0 = libm::floorf(fy) as i32;
    let tx = (fx - x0 as f32).clamp(0.0, 1.0);
    let ty = (fy - y0 as f32).clamp(0.0, 1.0);
    let at = |x: i32, y: i32| -> f32 {
        if x < 0 || y < 0 || x >= w || y >= h {
            0.0
        } else {
            f32::from(map[(y as usize) * (w as usize) + (x as usize)]) / 255.0
        }
    };
    let a = at(x0, y0);
    let b = at(x0 + 1, y0);
    let c = at(x0, y0 + 1);
    let d = at(x0 + 1, y0 + 1);
    a * (1.0 - tx) * (1.0 - ty) + b * tx * (1.0 - ty) + c * (1.0 - tx) * ty + d * tx * ty
}

/// Baked dial sample for an output pixel: sRGB gray 0..1, or `None` when
/// the scene carries no valid map (procedural fallback owns the pixel).
///
/// Geometry is output-driven, never radius-driven: the square image fills
/// the `min(width, height)` square centered on (`center_x`, `center_y`),
/// so a 600-image on a 600 screen maps texel 1:1 while a 400x300 screen
/// shows the square at x 50..349. Outside the square reads white (1.0).
pub fn dial_image_sample(scene: &super::scene::ClockScene<'_>, x: u32, y: u32) -> Option<f32> {
    let map = scene.dial?;
    if !map.validate() {
        return None;
    }
    let side = map.width as f32;
    let span = core::cmp::min(scene.width, scene.height) as f32;
    if span <= 0.0 {
        return None;
    }
    let dx = x as f32 - (scene.center_x - span / 2.0);
    let dy = y as f32 - (scene.center_y - span / 2.0);
    if dx < 0.0 || dy < 0.0 || dx >= span || dy >= span {
        return Some(1.0);
    }
    let w = map.width as i32;
    let h = map.height as i32;
    Some(bilinear_gray(
        map.pixels,
        w,
        h,
        (dx + 0.5) * side / span - 0.5,
        (dy + 0.5) * side / span - 0.5,
    ))
}

/// Albedo and specular strength for a dial-space point `p` (dial units
/// from the center, y down). Returns sRGB gray plus data specular.
pub fn dial_finish(px: f32, py: f32) -> (f32, f32) {
    let r = libm::sqrtf(px * px + py * py);
    if r < HUB_RADIUS {
        return (HUB, 0.6);
    }
    // Sixty minute ticks; every fifth is an hour tick, longer and wider.
    let angle = libm::atan2f(px, -py); // clockwise from 12, y down
    let slot = angle / (core::f32::consts::TAU / 60.0);
    let nearest = libm::roundf(slot);
    let frac = libm::fabsf(slot - nearest);
    let hour = libm::fabsf(nearest % 5.0) < 0.5;
    let (inner, half_width) = if hour { (0.82, 0.16) } else { (0.885, 0.07) };
    if r > inner && r < 0.965 && frac < half_width {
        return (TICK, DIAL_SPEC);
    }
    (PAPER, DIAL_SPEC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn paper_between_ticks() {
        // Halfway between two minute ticks at mid radius.
        let a = core::f32::consts::TAU / 120.0;
        let (albedo, _) = dial_finish(0.9 * libm::sinf(a), -0.9 * libm::cosf(a));
        assert!((albedo - PAPER).abs() < 1e-6);
    }

    #[test]
    fn twelve_oclock_tick_is_ink() {
        let (albedo, _) = dial_finish(0.0, -0.9);
        assert!((albedo - TICK).abs() < 1e-6);
    }

    #[test]
    fn hub_caps_the_center() {
        let (albedo, spec) = dial_finish(0.0, 0.0);
        assert!((albedo - HUB).abs() < 1e-6 && (spec - 0.6).abs() < 1e-6);
    }

    fn scene_with(pixels: &[u8], side: u32, w: u32, h: u32) -> super::super::scene::ClockScene<'_> {
        let mut scene = super::super::scene::ClockScene::for_size(w, h);
        scene.dial = Some(super::super::assets::DialMap {
            width: side,
            height: side,
            pixels,
        });
        scene.clamped()
    }

    #[test]
    fn native_size_maps_texel_one_to_one() {
        // 4x4 image on a 4x4 screen: output pixel reads its own texel.
        let pixels: Vec<u8> = (0..16u8).collect();
        let scene = scene_with(&pixels, 4, 4, 4);
        for y in 0..4 {
            for x in 0..4 {
                let got = dial_image_sample(&scene, x, y).expect("valid map");
                let want = f32::from((y * 4 + x) as u8) / 255.0;
                assert!((got - want).abs() < 1e-5, "({x},{y}): {got} vs {want}");
            }
        }
    }

    #[test]
    fn wide_screen_centers_square_and_whitens_margins() {
        // 2x2 image on a 4x2 screen: square spans x 1..2, margins white.
        let pixels = [0u8, 255, 255, 0];
        let scene = scene_with(&pixels, 2, 4, 2);
        assert_eq!(dial_image_sample(&scene, 0, 0), Some(1.0));
        assert_eq!(dial_image_sample(&scene, 3, 1), Some(1.0));
        // Inside the square the corners still land on texels.
        let inside = dial_image_sample(&scene, 1, 0).expect("inside");
        assert!((inside - 0.0).abs() < 1e-5);
    }

    #[test]
    fn upscale_bilinearly_interpolates_texels() {
        // 2x2 image (0, 255 / 255, 0) on a 4x4 screen: output (1,0) sits
        // a quarter texel across, interpolating the top row.
        let pixels = [0u8, 255, 255, 0];
        let scene = scene_with(&pixels, 2, 4, 4);
        let got = dial_image_sample(&scene, 1, 0).expect("valid map");
        assert!((got - 0.25).abs() < 1e-4, "bilinear interpolation: {got}");
    }

    #[test]
    fn half_size_averages_source_pixel_centers() {
        let pixels = [0u8, 255, 255, 0];
        let scene = scene_with(&pixels, 2, 1, 1);
        let got = dial_image_sample(&scene, 0, 0).expect("valid map");
        assert!((got - 0.5).abs() < 1e-4, "2x2 average: {got}");
    }

    #[test]
    fn invalid_map_falls_back_to_none() {
        // Wrong length, non-square, and empty maps all clamp to None.
        let pixels = [0u8; 8];
        let mut scene = super::super::scene::ClockScene::for_size(4, 4);
        scene.dial = Some(super::super::assets::DialMap {
            width: 4,
            height: 4,
            pixels: &pixels,
        });
        assert!(scene.clamped().dial.is_none());
        let non_square = [0u8; 6];
        scene.dial = Some(super::super::assets::DialMap {
            width: 3,
            height: 2,
            pixels: &non_square,
        });
        assert!(scene.clamped().dial.is_none());
        let empty: [u8; 0] = [];
        scene.dial = Some(super::super::assets::DialMap {
            width: 0,
            height: 0,
            pixels: &empty,
        });
        assert!(scene.clamped().dial.is_none());
        // No map at all samples nothing (procedural owns the pixel).
        let plain = super::super::scene::ClockScene::for_size(4, 4);
        assert_eq!(dial_image_sample(&plain, 0, 0), None);
    }
}
