use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, OriginDimensions, Pixel, Point, Size},
};

use super::sampling::{sample_sumi_sun_binary_pixel, sample_sumi_sun_gray4_level};
use super::*;

pub fn render_sumi_sun<T>(
    target: &mut T,
    params: SumiSunParams,
    mode: RenderMode,
    dither: DitherMode,
) where
    T: DrawTarget<Color = BinaryColor> + OriginDimensions,
{
    let size = target.size();
    let width = size.width as i32;
    let height = size.height as i32;
    if width <= 0 || height <= 0 {
        return;
    }

    let _ = target.clear(BinaryColor::Off);
    render_sumi_sun_rows_bw(width, height, 0..height, params, mode, dither, |x, y| {
        let _ = target.draw_iter(core::iter::once(Pixel(Point::new(x, y), BinaryColor::On)));
    });
}

pub fn render_sumi_sun_rows_bw<F>(
    width: i32,
    height: i32,
    rows: core::ops::Range<i32>,
    params: SumiSunParams,
    mode: RenderMode,
    dither: DitherMode,
    mut put_black_pixel: F,
) where
    F: FnMut(i32, i32),
{
    if width <= 0 || height <= 0 {
        return;
    }

    let y0 = rows.start.max(0);
    let y1 = rows.end.min(height).max(y0);
    let outer = params.radius_px.max(1) + 4;
    let x0 = (params.center.x - outer).max(0);
    let x1 = (params.center.x + outer + 1).min(width).max(x0);
    let yy0 = y0.max(params.center.y - outer);
    let yy1 = y1.min(params.center.y + outer + 1).max(yy0);
    for y in yy0..yy1 {
        for x in x0..x1 {
            if sample_sumi_sun_binary_pixel(x, y, params, mode, dither) == BinaryColor::On {
                put_black_pixel(x, y);
            }
        }
    }
}

pub fn render_sumi_sun_gray4_packed(
    framebuffer: &mut [u8],
    size: Size,
    params: SumiSunParams,
    dither: DitherMode,
) -> bool {
    let width = size.width as usize;
    let height = size.height as usize;
    let required = (width * height).div_ceil(2);
    if framebuffer.len() < required {
        return false;
    }

    framebuffer[..required].fill(0x00);
    for y in 0..height {
        for x in 0..width {
            let level = sample_sumi_sun_gray4_level(x as i32, y as i32, params, dither);
            let idx = y * width + x;
            let byte_idx = idx >> 1;
            if (idx & 1) == 0 {
                framebuffer[byte_idx] = (level << 4) | (framebuffer[byte_idx] & 0x0F);
            } else {
                framebuffer[byte_idx] = (framebuffer[byte_idx] & 0xF0) | level;
            }
        }
    }

    true
}
