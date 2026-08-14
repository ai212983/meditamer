#[cfg(test)]
#[path = "dither/tests.rs"]
mod tests;

#[path = "dither/types.rs"]
mod types;

pub(crate) use types::DirtyArea;

const WIDTH: usize = 600;
const HEIGHT: usize = 600;
const ROW_BYTES: usize = WIDTH / 8;
const FRAMEBUFFER_BYTES: usize = ROW_BYTES * HEIGHT;
pub(crate) unsafe fn blit_l8(area: DirtyArea, pixels: *const u8, framebuffer: &mut [u8]) -> bool {
    let width = area.x2.saturating_sub(area.x1).saturating_add(1);
    let height = area.y2.saturating_sub(area.y1).saturating_add(1);
    if width <= 0 || height <= 0 || pixels.is_null() || framebuffer.len() != FRAMEBUFFER_BYTES {
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
    let bitmap = unsafe { core::slice::from_raw_parts(pixels, bitmap_len) };
    blit_l8_slice(area, bitmap, width, framebuffer)
}

fn blit_l8_slice(area: DirtyArea, bitmap: &[u8], stride: usize, framebuffer: &mut [u8]) -> bool {
    let x_start = area.x1.max(0).min(WIDTH as i32) as usize;
    let y_start = area.y1.max(0).min(HEIGHT as i32) as usize;
    let x_end = area.x2.saturating_add(1).max(0).min(WIDTH as i32) as usize;
    let y_end = area.y2.saturating_add(1).max(0).min(HEIGHT as i32) as usize;
    if x_start >= x_end || y_start >= y_end || stride == 0 {
        return false;
    }

    let first_y_group = y_start / 8;
    let last_y_group = (y_end - 1) / 8;
    for x in x_start..x_end {
        let source_column = (x as i32 - area.x1) as usize;
        for y_group in first_y_group..=last_y_group {
            let group_start = y_group * 8;
            let group_end = (group_start + 8).min(y_end);
            let logical_y_start = group_start.max(y_start);
            let mut destination_mask = 0u8;
            let mut black_bits = 0u8;

            for y in logical_y_start..group_end {
                let source_row = (y as i32 - area.y1) as usize;
                let source_index = source_row * stride + source_column;
                let Some(&luminance) = bitmap.get(source_index) else {
                    return false;
                };
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
