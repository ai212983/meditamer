//! Packs an LVGL L8 buffer into the Inkplate panel's 1bpp framebuffer.
//!
//! This is board code, not rendering: the destination is column-major
//! (`ROW_BYTES * x`), bottom-up, and packs eight rows per byte along Y, which
//! is the ED038TH2 panel's layout rather than anything general. ADR-0015 moved
//! it here after briefly filing it under `platform/ui/render` on the strength of
//! it having no imports -- which says nothing about coupling to a hardware
//! memory format.

#[cfg(test)]
#[path = "panel_blit/tests.rs"]
mod tests;

use render::DirtyArea;

use super::{
    E_INK_HEIGHT as HEIGHT, E_INK_WIDTH as WIDTH, FRAMEBUFFER_BYTES, GRAYSCALE_FRAMEBUFFER_BYTES,
};
use crate::frame::level_from_luminance;

const ROW_BYTES: usize = WIDTH / 8;
/// Gray4 counterpart of [`ROW_BYTES`]: same column-major organisation, but
/// two pixels per byte along Y instead of eight.
const GRAY4_COL_BYTES: usize = HEIGHT / 2;
/// Blit an LVGL L8 (8-bit grayscale) buffer into a 1bpp framebuffer.
///
/// Returns `false` without touching `framebuffer` when `area` is empty,
/// `pixels` is shorter than the area's width x height, or `framebuffer` is not
/// exactly [`FRAMEBUFFER_BYTES`].
pub fn blit_l8(area: DirtyArea, pixels: &[u8], framebuffer: &mut [u8]) -> bool {
    let width = i64::from(area.x2) - i64::from(area.x1) + 1;
    let height = i64::from(area.y2) - i64::from(area.y1) + 1;
    if width <= 0 || height <= 0 || framebuffer.len() != FRAMEBUFFER_BYTES {
        return false;
    }

    let Ok(width) = usize::try_from(width) else {
        return false;
    };
    let Ok(height) = usize::try_from(height) else {
        return false;
    };
    let Some(bitmap_len) = width.checked_mul(height) else {
        return false;
    };
    if pixels.len() < bitmap_len {
        return false;
    }

    let x_start = area.x1.max(0).min(WIDTH as i32) as usize;
    let y_start = area.y1.max(0).min(HEIGHT as i32) as usize;
    let x_end = area.x2.saturating_add(1).max(0).min(WIDTH as i32) as usize;
    let y_end = area.y2.saturating_add(1).max(0).min(HEIGHT as i32) as usize;
    if x_start >= x_end || y_start >= y_end {
        return false;
    }

    let first_y_group = y_start / 8;
    let last_y_group = (y_end - 1) / 8;
    for x in x_start..x_end {
        let source_column = (x as i64 - i64::from(area.x1)) as usize;
        for y_group in first_y_group..=last_y_group {
            let group_start = y_group * 8;
            let group_end = (group_start + 8).min(y_end);
            let logical_y_start = group_start.max(y_start);
            let mut destination_mask = 0u8;
            let mut black_bits = 0u8;

            for y in logical_y_start..group_end {
                let source_row = (y as i64 - i64::from(area.y1)) as usize;
                let source_index = source_row * width + source_column;
                let luminance = pixels[source_index];
                let panel_bit = 1u8 << ((HEIGHT - 1 - y) % 8);
                destination_mask |= panel_bit;
                if dithered_black(luminance) {
                    black_bits |= panel_bit;
                }
            }

            let panel_byte = (HEIGHT - 1 - group_start) / 8;
            let destination = ROW_BYTES * x + panel_byte;
            framebuffer[destination] = (framebuffer[destination] & !destination_mask) | black_bits;
        }
    }
    true
}

#[inline(always)]
fn dithered_black(luminance: u8) -> bool {
    luminance < 128
}

/// Blit an LVGL L8 buffer into a **Gray4** framebuffer, preserving the panel's
/// eight physical grey levels instead of thresholding to black and white.
///
/// The destination layout is the Gray4 analogue of [`blit_l8`]'s: column-major
/// with Y inverted, `GRAY4_COL_BYTES * x + (HEIGHT - 1 - y) / 2`, two pixels
/// per byte, the **even** inverted-Y in the high nibble. `blit_l8` is the
/// authority for this organisation because it is the path the shipping UI
/// already renders through; the unit tests additionally pin this function
/// against `frame::rotate_gray4`, which was verified on the panel itself.
///
/// LVGL's L8 and the panel's levels run the same direction -- higher is
/// lighter -- so no inversion is needed here; see
/// [`crate::frame::level_from_luminance`].
///
/// Returns `false` without touching `framebuffer` under the same conditions
/// as [`blit_l8`].
pub fn blit_l8_gray4(area: DirtyArea, pixels: &[u8], framebuffer: &mut [u8]) -> bool {
    let width = i64::from(area.x2) - i64::from(area.x1) + 1;
    let height = i64::from(area.y2) - i64::from(area.y1) + 1;
    if width <= 0 || height <= 0 || framebuffer.len() != GRAYSCALE_FRAMEBUFFER_BYTES {
        return false;
    }
    let (Ok(width), Ok(height)) = (usize::try_from(width), usize::try_from(height)) else {
        return false;
    };
    let Some(bitmap_len) = width.checked_mul(height) else {
        return false;
    };
    if pixels.len() < bitmap_len {
        return false;
    }

    let x_start = area.x1.max(0).min(WIDTH as i32) as usize;
    let y_start = area.y1.max(0).min(HEIGHT as i32) as usize;
    let x_end = area.x2.saturating_add(1).max(0).min(WIDTH as i32) as usize;
    let y_end = area.y2.saturating_add(1).max(0).min(HEIGHT as i32) as usize;
    if x_start >= x_end || y_start >= y_end {
        return false;
    }

    for x in x_start..x_end {
        let source_column = (x as i64 - i64::from(area.x1)) as usize;
        for y in y_start..y_end {
            let source_row = (y as i64 - i64::from(area.y1)) as usize;
            let luminance = pixels[source_row * width + source_column];
            let level = level_from_luminance(luminance);
            let panel_y = HEIGHT - 1 - y;
            let destination = GRAY4_COL_BYTES * x + panel_y / 2;
            let slot = &mut framebuffer[destination];
            if panel_y.is_multiple_of(2) {
                *slot = (*slot & 0x0f) | (level << 4);
            } else {
                *slot = (*slot & 0xf0) | (level & 0x0f);
            }
        }
    }
    true
}
