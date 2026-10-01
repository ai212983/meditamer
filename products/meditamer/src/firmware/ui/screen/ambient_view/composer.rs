//! Bit-plane composer for the Ambient Home sky/sun surface.
//!
//! This module has no LVGL and no crate-path dependency: it only uses
//! `core`, so it can be pulled unmodified into a host test harness (see
//! `test-support/host/ui_shell_host_harness`) via `#[path]`. Every item
//! uses `pub` rather than a `pub(in crate::firmware::ui)` path restriction
//! for exactly that reason -- the restriction is resolved against whichever
//! crate the file is compiled into.
//!
//! The frame is a 600x600 MSB-first bit plane (`1` = ink), matching the
//! `SKY.BIN` pack layout and the canvas publisher (`write_ink_bits`
//! expands set bits to `0x00` ink / clear bits to `0xFF` paper). The
//! composition order is sky, arc guide, clipped sun sprite, then opaque mountain
//! pixels over the lower canvas. Eligible mountain paper clears sun/sky
//! ink; transparent edges leave the underlying scene untouched.
//! The guide and the sun use the same curve control points.

#![forbid(unsafe_code)]

/// Surface dimensions the pack covers.
pub const WIDTH: usize = 600;
pub const HEIGHT: usize = 600;
/// Packed bytes of one full frame.
pub const FRAME_LEN: usize = WIDTH * HEIGHT / 8; // 45_000

/// Frozen Mountain band geometry: the lower 269 full-width rows.
pub const MOUNTAIN_OVERLAY_ROWS: usize = 269;
/// First frame row of the frozen Mountain band (`HEIGHT - ROWS`).
pub const MOUNTAIN_OVERLAY_FIRST_ROW: usize = HEIGHT - MOUNTAIN_OVERLAY_ROWS; // 331
/// Packed bytes of one Mountain overlay row (`WIDTH / 8`).
pub const MOUNTAIN_OVERLAY_ROW_BYTES: usize = WIDTH / 8; // 75
/// Packed bytes of one Mountain overlay plane (`ROWS * ROW_BYTES`).
///
/// The firmware overlay path sizes buffers through
/// `mountain_assets::OVERLAY_*` (storage cannot depend on this UI
/// module); these spellings stay for the host equivalence tests, which
/// pin both equal, so device-only dead code is allowed here.
#[cfg_attr(target_os = "none", allow(dead_code))]
pub const MOUNTAIN_OVERLAY_PLANE_BYTES: usize = MOUNTAIN_OVERLAY_ROWS * MOUNTAIN_OVERLAY_ROW_BYTES; // 20_175
/// Total streamed Mountain overlay bytes: 269 ink rows immediately
/// followed by 269 eligibility/opacity rows.
#[cfg_attr(target_os = "none", allow(dead_code))]
pub const MOUNTAIN_OVERLAY_LEN: usize = 2 * MOUNTAIN_OVERLAY_PLANE_BYTES; // 40_350

/// Rounds to the nearest integer (half away from zero). `core` has no
/// `f32::round` without `std`/`libm`, so this is the manual equivalent;
/// identical to the conversion the vector arc/circle path used, so sprite
/// centers land where the circle center used to.
pub fn round_to_i32(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

/// Reads one MSB-first bit. Out-of-range coordinates read clear (paper),
/// so clipped sprite edges need no separate bounds pass.
pub fn bit_at(plane: &[u8], stride: usize, x: usize, y: usize) -> bool {
    let byte = y
        .checked_mul(stride)
        .and_then(|row| row.checked_add(x / 8))
        .and_then(|at| plane.get(at).copied())
        .unwrap_or(0);
    byte & (0x80 >> (x % 8)) != 0
}

/// Copies the sky plane over the frame. Returns `false` (frame untouched)
/// when either side is not exactly one full frame.
pub fn paint_sky(frame: &mut [u8], sky: &[u8]) -> bool {
    if frame.len() != FRAME_LEN || sky.len() != FRAME_LEN {
        return false;
    }
    frame.copy_from_slice(sky);
    true
}

/// Paint a one-pixel ink guide along the sun's cubic Bezier path. The 48
/// segments match the former vector arc's sampling; off-screen endpoints
/// are clipped per pixel, without a second frame or any heap allocation.
pub fn paint_arc_guide(frame: &mut [u8], points: [[f32; 2]; 4]) -> bool {
    if frame.len() != FRAME_LEN || !points.iter().flatten().all(|v| v.is_finite()) {
        return false;
    }
    let point_at = |t: f32| {
        let mt = 1.0 - t;
        let weights = [mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t];
        let x = points.iter().zip(weights).map(|(p, w)| p[0] * w).sum();
        let y = points.iter().zip(weights).map(|(p, w)| p[1] * w).sum();
        (round_to_i32(x), round_to_i32(y))
    };
    let mut previous = point_at(0.0);
    for step in 1..48 {
        let next = point_at(step as f32 / 47.0);
        let (mut x, mut y) = previous;
        let dx = (next.0 - x).abs();
        let sx = if x < next.0 { 1 } else { -1 };
        let dy = -(next.1 - y).abs();
        let sy = if y < next.1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) {
                let at = y as usize * (WIDTH / 8) + x as usize / 8;
                frame[at] |= 0x80 >> (x as usize % 8);
            }
            if (x, y) == next {
                break;
            }
            let twice = 2 * error;
            if twice >= dy {
                error += dy;
                x += sx;
            }
            if twice <= dx {
                error += dx;
                y += sy;
            }
        }
        previous = next;
    }
    true
}

/// Borrowed sun sprite planes, decoded once per composition from the boot
/// pack bytes.
pub struct SunSprite<'a> {
    /// Ink bits, MSB-first rows of `stride` bytes.
    pub ink: &'a [u8],
    /// Mask bits, same layout; `1` = opaque.
    pub mask: &'a [u8],
    /// Sprite width in pixels.
    pub width: usize,
    /// Sprite height in pixels.
    pub height: usize,
    /// Row stride in bytes.
    pub stride: usize,
    /// Mask centroid in sprite pixels; the composer centers this on the
    /// sun path point.
    pub anchor: (f32, f32),
}

/// Composites the sun sprite with its top-left at `round(center - anchor)`
/// (the anchor lands on the sun path point, matching the host review).
/// Mask-clear pixels keep whatever is underneath. Returns `false` (frame
/// untouched) on geometry the pack decoder itself would have rejected.
pub fn paint_sun(frame: &mut [u8], sprite: &SunSprite<'_>, center: [f32; 2]) -> bool {
    if frame.len() != FRAME_LEN
        || sprite.width == 0
        || sprite.height == 0
        || sprite.stride != sprite.width.div_ceil(8)
        || sprite.ink.len() != sprite.stride * sprite.height
        || sprite.mask.len() != sprite.stride * sprite.height
    {
        return false;
    }
    let x0 = round_to_i32(center[0] - sprite.anchor.0);
    let y0 = round_to_i32(center[1] - sprite.anchor.1);
    let frame_stride = WIDTH / 8;
    for y in 0..sprite.height {
        for x in 0..sprite.width {
            if !bit_at(sprite.mask, sprite.stride, x, y) {
                continue;
            }
            let Some(px) = x0.checked_add(x as i32) else {
                continue;
            };
            let Some(py) = y0.checked_add(y as i32) else {
                continue;
            };
            if px < 0 || py < 0 || px >= WIDTH as i32 || py >= HEIGHT as i32 {
                continue;
            }
            let at = py as usize * frame_stride + px as usize / 8;
            let mask = 0x80 >> ((px as usize) % 8);
            // `frame` is exactly FRAME_LEN, so `at` is always in range;
            // the `if let` keeps the no-panic contract if that changes.
            if let Some(byte) = frame.get_mut(at) {
                if bit_at(sprite.ink, sprite.stride, x, y) {
                    *byte |= mask;
                } else {
                    *byte &= !mask;
                }
            }
        }
    }
    true
}

/// Overlay one validated mountain pack at temperature-derived coverage.
/// All work is bounded to the pack's stored row band; the only scratch is
/// a 256-bin histogram and one 75-byte row. Returns false before changing
/// any frame bytes when geometry is invalid.
///
/// The firmware scene merges precomposed overlays
/// ([`merge_mountain_overlay`]) instead; this whole-pack painter stays as
/// the host equivalence oracle (the streamed-overlay test proves both
/// paths identical), so device-only dead code is allowed here.
#[cfg_attr(target_os = "none", allow(dead_code))]
pub fn paint_mountain(frame: &mut [u8], pack: &mountain_snow::pack::Pack<'_>, percent: u8) -> bool {
    let header = pack.header();
    if frame.len() != FRAME_LEN
        || percent > 100
        || header.first_row + header.rows > HEIGHT
        || header.rows == 0
    {
        return false;
    }
    let mut histogram = [0u32; 256];
    let mut eligible_count = 0u32;
    for row in 0..header.rows {
        let (Ok(barrier), Ok(eligible)) = (pack.barrier_row(row), pack.eligible_row(row)) else {
            return false;
        };
        for x in 0..WIDTH {
            if eligible[x / 8] & (0x80 >> (x % 8)) != 0 {
                histogram[barrier[x] as usize] += 1;
                eligible_count += 1;
            }
        }
    }
    let target = (eligible_count * u32::from(percent) + 50) / 100;
    let cut = mountain_snow::composer::cut_from_histogram(&histogram, target);
    let mut row_bits = [0u8; WIDTH / 8];
    for row in 0..header.rows {
        let (Ok(rock), Ok(snow), Ok(barrier), Ok(eligible), Ok(noise)) = (
            pack.rock_row(row),
            pack.snow_row(row),
            pack.barrier_row(row),
            pack.eligible_row(row),
            pack.noise_row(row),
        ) else {
            return false;
        };
        let inputs = mountain_snow::composer::RowInputs {
            rock,
            snow,
            barrier,
            eligible,
            noise,
        };
        if mountain_snow::composer::compose_row_for_percent(
            &inputs,
            percent,
            cut,
            64,
            &mut row_bits,
        )
        .is_err()
        {
            return false;
        }
        let offset = (header.first_row + row) * (WIDTH / 8);
        let destination = &mut frame[offset..offset + WIDTH / 8];
        for ((base, &opacity), &ink) in destination.iter_mut().zip(eligible).zip(&row_bits) {
            *base = (*base & !opacity) | (ink & opacity);
        }
    }
    true
}

/// Merge one streamed Mountain overlay over the frame band
/// `first_row..first_row + rows`.
///
/// Layout is the frozen representation: `rows` packed ink rows of
/// [`MOUNTAIN_OVERLAY_ROW_BYTES`] bytes immediately followed by `rows`
/// packed eligibility/opacity rows. Each packed byte applies exactly the
/// legacy formula `base = (base & !opacity) | (ink & opacity)`.
///
/// Returns `false` (frame untouched) unless the frame is exactly one full
/// frame, `rows` is nonzero with `first_row + rows <= HEIGHT`, and the
/// overlay holds exactly `2 * rows * ROW_BYTES` bytes.
pub fn merge_mountain_overlay(
    frame: &mut [u8],
    overlay: &[u8],
    first_row: usize,
    rows: usize,
) -> bool {
    if frame.len() != FRAME_LEN || rows == 0 {
        return false;
    }
    let Some(band_end) = first_row.checked_add(rows) else {
        return false;
    };
    if band_end > HEIGHT {
        return false;
    }
    let Some(plane_len) = rows.checked_mul(MOUNTAIN_OVERLAY_ROW_BYTES) else {
        return false;
    };
    let Some(expected_len) = plane_len.checked_mul(2) else {
        return false;
    };
    if overlay.len() != expected_len {
        return false;
    }
    let (ink_plane, opacity_plane) = overlay.split_at(plane_len);
    let stride = MOUNTAIN_OVERLAY_ROW_BYTES;
    for row in 0..rows {
        let frame_row = &mut frame[(first_row + row) * stride..(first_row + row + 1) * stride];
        let ink_row = &ink_plane[row * stride..(row + 1) * stride];
        let opacity_row = &opacity_plane[row * stride..(row + 1) * stride];
        for ((base, &ink), &opacity) in frame_row.iter_mut().zip(ink_row).zip(opacity_row) {
            *base = (*base & !opacity) | (ink & opacity);
        }
    }
    true
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn sky_paint_is_verbatim_and_sized() {
        let mut frame = [0u8; FRAME_LEN];
        let mut sky = [0u8; FRAME_LEN];
        sky[0] = 0xA5;
        sky[FRAME_LEN - 1] = 0x5A;
        assert!(paint_sky(&mut frame, &sky));
        assert_eq!(frame, sky);
        assert!(!paint_sky(&mut frame[..FRAME_LEN - 1], &sky));
    }

    #[test]
    fn arc_guide_follows_curve_and_sun_can_cover_it() {
        let mut frame = [0u8; FRAME_LEN];
        let points = [[-48.0, 378.0], [105.0, 15.0], [495.0, 15.0], [648.0, 378.0]];
        assert!(paint_arc_guide(&mut frame, points));
        assert!(bit_at(&frame, WIDTH / 8, 300, 106));
        assert!((250..320).any(|y| bit_at(&frame, WIDTH / 8, 0, y)));
        assert!((250..320).any(|y| bit_at(&frame, WIDTH / 8, 599, y)));
        let ink = [0u8; 1];
        let mask = [0x80u8; 1];
        let sprite = SunSprite {
            ink: &ink,
            mask: &mask,
            width: 1,
            height: 1,
            stride: 1,
            anchor: (0.0, 0.0),
        };
        assert!(paint_sun(&mut frame, &sprite, [300.0, 106.0]));
        assert!(!bit_at(&frame, WIDTH / 8, 300, 106));
        assert!(!paint_arc_guide(&mut frame[..FRAME_LEN - 1], points));
    }

    #[test]
    fn sun_centers_anchor_and_respects_mask() {
        // Frame starts paper-clear; the sprite paints paper (ink clear) so
        // only mask-opaque pixels change.
        let mut frame = [0xFFu8; FRAME_LEN];
        let ink = [0x00u8; 8];
        let mut mask = [0u8; 8];
        for slot in mask[2..6].iter_mut() {
            *slot = 0x3C; // opaque x 2..5
        }
        let sprite = SunSprite {
            ink: &ink,
            mask: &mask,
            width: 8,
            height: 8,
            stride: 1,
            anchor: (3.5, 3.5),
        };
        // Center (100.5, 100.5) minus anchor (3.5, 3.5) puts the sprite at
        // top-left (97, 97); the opaque 4x4 clears x/y 99..102.
        assert!(paint_sun(&mut frame, &sprite, [100.5, 100.5]));
        assert!(!bit_at(&frame, WIDTH / 8, 99, 99));
        assert!(!bit_at(&frame, WIDTH / 8, 102, 102));
        assert!(bit_at(&frame, WIDTH / 8, 98, 98));
        assert!(bit_at(&frame, WIDTH / 8, 103, 103));
    }

    #[test]
    fn sun_off_surface_clips_without_panic() {
        let mut frame = [0u8; FRAME_LEN];
        let ink = [0xFFu8; 8];
        let sprite = SunSprite {
            ink: &ink,
            mask: &ink,
            width: 8,
            height: 8,
            stride: 1,
            anchor: (4.0, 4.0),
        };
        assert!(paint_sun(&mut frame, &sprite, [-100.0, -100.0]));
        assert!(frame.iter().all(|&b| b == 0));
    }

    #[test]
    fn bad_sprite_geometry_leaves_frame_untouched() {
        let mut frame = [0xFFu8; FRAME_LEN];
        let plane = [0xFFu8; 8];
        let sprite = SunSprite {
            ink: &plane,
            mask: &plane,
            width: 8,
            height: 8,
            stride: 2, // wrong stride for 8px rows
            anchor: (4.0, 4.0),
        };
        assert!(!paint_sun(&mut frame, &sprite, [100.0, 100.0]));
        assert!(frame.iter().all(|&b| b == 0xFF));
    }
}
