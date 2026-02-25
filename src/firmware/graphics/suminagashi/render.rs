use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, OriginDimensions, Pixel, Point, Size},
};

use super::*;

pub fn render_seeded_inverse_rgss<T>(
    target: &mut T,
    seed: u32,
    rgss: RgssMode,
    mode: RenderMode,
    dither: DitherMode,
) where
    T: DrawTarget<Color = BinaryColor> + OriginDimensions,
{
    let size = target.size();
    let scene = build_seeded_scene(seed, size);
    let _ = target.clear(BinaryColor::Off);

    for y in 0..size.height as i32 {
        for x in 0..size.width as i32 {
            if sample_binary_pixel(&scene, x, y, rgss, mode, dither) == BinaryColor::On {
                let _ =
                    target.draw_iter(core::iter::once(Pixel(Point::new(x, y), BinaryColor::On)));
            }
        }
    }
}

pub fn render_seeded_inverse_rgss_bw<F>(
    width: i32,
    height: i32,
    seed: u32,
    rgss: RgssMode,
    mode: RenderMode,
    dither: DitherMode,
    mut put_black_pixel: F,
) where
    F: FnMut(i32, i32),
{
    if width <= 0 || height <= 0 {
        return;
    }

    let size = Size::new(width as u32, height as u32);
    let scene = build_seeded_scene(seed, size);
    for y in 0..height {
        for x in 0..width {
            if sample_binary_pixel(&scene, x, y, rgss, mode, dither) == BinaryColor::On {
                put_black_pixel(x, y);
            }
        }
    }
}

pub fn render_scene_rows_bw<F>(
    scene: &MarblingScene,
    width: i32,
    rows: core::ops::Range<i32>,
    style: SceneRenderStyle,
    mut put_black_pixel: F,
) where
    F: FnMut(i32, i32),
{
    let y0 = rows.start.max(0);
    let y1 = rows.end.max(y0);
    for y in y0..y1 {
        for x in 0..width {
            if sample_binary_pixel(scene, x, y, style.rgss, style.mode, style.dither)
                == BinaryColor::On
            {
                put_black_pixel(x, y);
            }
        }
    }
}

pub fn render_scene_rows_bw_masked<M, F>(
    scene: &MarblingScene,
    width: i32,
    rows: core::ops::Range<i32>,
    style: SceneRenderStyle,
    mut include_pixel: M,
    mut put_black_pixel: F,
) where
    M: FnMut(i32, i32) -> bool,
    F: FnMut(i32, i32),
{
    let y0 = rows.start.max(0);
    let y1 = rows.end.max(y0);
    for y in y0..y1 {
        for x in 0..width {
            if !include_pixel(x, y) {
                continue;
            }
            if sample_binary_pixel(scene, x, y, style.rgss, style.mode, style.dither)
                == BinaryColor::On
            {
                put_black_pixel(x, y);
            }
        }
    }
}

pub fn render_seeded_gray4_packed(
    framebuffer: &mut [u8],
    size: Size,
    seed: u32,
    rgss: RgssMode,
    dither: DitherMode,
) -> bool {
    let width = size.width as usize;
    let height = size.height as usize;
    let required = (width * height).div_ceil(2);
    if framebuffer.len() < required {
        return false;
    }

    framebuffer[..required].fill(0x00);
    let scene = build_seeded_scene(seed, size);

    for y in 0..height {
        for x in 0..width {
            let level = sample_gray4_level(&scene, x as i32, y as i32, rgss, dither);
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
