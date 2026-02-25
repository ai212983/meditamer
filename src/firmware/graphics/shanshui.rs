use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, OriginDimensions, Pixel, Point},
};
use fixed::types::I16F16;

pub type Fx = I16F16;

const FX_ZERO: Fx = Fx::from_bits(0);
const FX_HALF: Fx = Fx::from_bits(1 << 15);
const FX_ONE: Fx = Fx::from_bits(1 << 16);
const FX_TWO: Fx = Fx::from_bits(2 << 16);
const FX_THREE: Fx = Fx::from_bits(3 << 16);
const FX_0_35: Fx = Fx::from_bits(22_938);
const FX_0_4: Fx = Fx::from_bits(26_214);
const FX_0_45: Fx = Fx::from_bits(29_491);
const FX_0_6: Fx = Fx::from_bits(39_322);
const MAX_ATKINSON_WIDTH: usize = 600;

#[derive(Clone, Copy)]
struct TreeShape {
    root_x: i32,
    root_y: i32,
    trunk_top: i32,
    trunk_w: i32,
    crown_x: i32,
    crown_y: i32,
    chunk_left: i32,
}

mod helpers;
use helpers::*;

pub fn render_shanshui<T>(target: &mut T, seed: u32)
where
    T: DrawTarget<Color = BinaryColor> + OriginDimensions,
{
    let size = target.size();
    let width = size.width as i32;
    let height = size.height as i32;
    if width <= 0 || height <= 0 {
        return;
    }

    let _ = target.clear(BinaryColor::Off);
    render_shanshui_rows_bw(width, height, 0, height, seed, |x, y| {
        let _ = target.draw_iter(core::iter::once(Pixel(Point::new(x, y), BinaryColor::On)));
    });
}

pub fn render_shanshui_rows_bw<F>(
    width: i32,
    height: i32,
    y_start: i32,
    y_end: i32,
    seed: u32,
    mut put_black_pixel: F,
) where
    F: FnMut(i32, i32),
{
    if width <= 0 || height <= 0 {
        return;
    }

    let y0 = y_start.max(0);
    let y1 = y_end.min(height).max(y0);
    for y in y0..y1 {
        for x in 0..width {
            if sample_shanshui_pixel(x, y, width, height, seed) {
                put_black_pixel(x, y);
            }
        }
    }
}

#[inline]
fn sample_shanshui_pixel(x: i32, y: i32, width: i32, height: i32, seed: u32) -> bool {
    let ink = sample_shanshui_ink_u8(x, y, width, height, seed);
    let grain = hash_xy(x, y, seed ^ 0xD4E1_2A91) as u8;
    grain < ink
}

#[inline]
fn sample_shanshui_ink_u8(x: i32, y: i32, width: i32, height: i32, seed: u32) -> u8 {
    let far_h = layer_height(x, width, height, seed ^ 0x4A56_CE3D, 7, FX_0_35, FX_0_35);
    let mid_h = layer_height(
        x + 67,
        width,
        height,
        seed ^ 0x891E_2B6F,
        6,
        FX_0_45,
        FX_0_4,
    );
    let near_h = layer_height(
        x + 131,
        width,
        height,
        seed ^ 0x2D9F_7A43,
        5,
        FX_0_6,
        FX_0_45,
    );
    let surfaces = [far_h, mid_h, near_h];
    let depth_factor = depth_factor_from_surfaces(y, &surfaces, height);

    if !river_tree_exclusion(x, y, width, height, seed) {
        if tree_halo(x, y, width, height, near_h, seed) {
            return 0;
        }
        if tree_ink(x, y, width, height, near_h, seed) {
            return 190;
        }
    }

    let (layer, top) = if y >= near_h {
        (2u8, near_h)
    } else if y >= mid_h {
        (1u8, mid_h)
    } else if y >= far_h {
        (0u8, far_h)
    } else {
        return river_override(0, x, y, width, height, seed);
    };

    let depth = (y - top).max(0);
    let slope = slope_abs(x, width, height, seed, layer);
    let ridge = ridge_noise(
        x,
        y,
        seed ^ (layer as u32).wrapping_mul(0x9E37_79B9),
        4 + layer,
    );

    // Unified depth: mountain tone and river geometry both derive from `depth_factor`.
    let base = lerp_u16(72, 176, depth_factor);
    let slope_gain = lerp_u16(1, 3, depth_factor);
    let depth_gain = lerp_u16(4, 9, depth_factor);
    let ridge_gain = lerp_u16(22, 38, depth_factor);
    let slope_boost = (slope.min(10) as u16) * slope_gain;
    let depth_boost = ((depth.min(140) as u16) * depth_gain) / 140;
    let ridge_boost = (((ridge.to_bits().max(0) as u32) * ridge_gain as u32) >> 16) as u16;
    let mut threshold = (base + slope_boost + depth_boost + ridge_boost).min(255) as i32;
    // Reduce blotchy fill on flat areas while preserving dark strokes on steeper regions.
    let flat_penalty = (10 - slope.min(10)) * 2;
    threshold = (threshold - flat_penalty).clamp(0, 255);

    // Inject a small 2D wash variation so near-field tone does not form vertical curtains.
    let wash = value_noise2d(
        x.saturating_mul(2).saturating_add(y >> 1),
        y.saturating_mul(2).saturating_sub(x >> 2),
        seed ^ 0xE19B_4C73,
        5,
    );
    let wash_shift = (((wash.to_bits() - FX_HALF.to_bits()) as i64 * 18) >> 16) as i32;
    threshold = (threshold + wash_shift).clamp(0, 255);

    // Atmospheric perspective from the same depth field.
    let inv_depth = (FX_ONE - depth_factor).clamp(FX_ZERO, FX_ONE);
    let haze = value_noise2d(
        x.saturating_sub(y >> 2),
        y.saturating_add(x >> 3),
        seed ^ 0x4FD7_A2B1,
        6,
    );
    let haze_mix = (haze * inv_depth * inv_depth).clamp(FX_ZERO, FX_ONE);
    let haze_penalty = (((haze_mix.to_bits().max(0) as u32) * 56) >> 16) as i32;
    threshold = (threshold - haze_penalty - 6).clamp(0, 255).min(210);

    river_override(threshold as u8, x, y, width, height, seed)
}

pub fn render_shanshui_bw_atkinson<F>(width: i32, height: i32, seed: u32, mut put_black_pixel: F)
where
    F: FnMut(i32, i32),
{
    if width <= 0 || height <= 0 {
        return;
    }

    let w = width as usize;
    if w > MAX_ATKINSON_WIDTH {
        render_shanshui_rows_bw(width, height, 0, height, seed, put_black_pixel);
        return;
    }

    let mut err_row0 = [0i16; MAX_ATKINSON_WIDTH + 4];
    let mut err_row1 = [0i16; MAX_ATKINSON_WIDTH + 4];
    let mut err_row2 = [0i16; MAX_ATKINSON_WIDTH + 4];

    for y in 0..height {
        if (y & 1) == 0 {
            let mut x = 0usize;
            while x < w {
                let xi = x + 2;
                let base = sample_shanshui_ink_u8(x as i32, y, width, height, seed) as i16;
                let ink = (base + err_row0[xi]).clamp(0, 255);
                let quantized = if ink >= 142 { 255i16 } else { 0i16 };

                if quantized != 0 {
                    put_black_pixel(x as i32, y);
                }

                let error = ink - quantized;
                if error != 0 {
                    let e = error >> 4;
                    atkinson_add(&mut err_row0[xi + 1], e);
                    atkinson_add(&mut err_row0[xi + 2], e);
                    atkinson_add(&mut err_row1[xi - 1], e);
                    atkinson_add(&mut err_row1[xi], e);
                    atkinson_add(&mut err_row1[xi + 1], e);
                    atkinson_add(&mut err_row2[xi], e);
                }

                x += 1;
            }
        } else {
            let mut x = w;
            while x > 0 {
                x -= 1;
                let xi = x + 2;
                let base = sample_shanshui_ink_u8(x as i32, y, width, height, seed) as i16;
                let ink = (base + err_row0[xi]).clamp(0, 255);
                let quantized = if ink >= 142 { 255i16 } else { 0i16 };

                if quantized != 0 {
                    put_black_pixel(x as i32, y);
                }

                let error = ink - quantized;
                if error != 0 {
                    let e = error >> 4;
                    atkinson_add(&mut err_row0[xi - 1], e);
                    atkinson_add(&mut err_row0[xi - 2], e);
                    atkinson_add(&mut err_row1[xi + 1], e);
                    atkinson_add(&mut err_row1[xi], e);
                    atkinson_add(&mut err_row1[xi - 1], e);
                    atkinson_add(&mut err_row2[xi], e);
                }
            }
        }

        let mut i = 0usize;
        let n = w + 4;
        while i < n {
            err_row0[i] = err_row1[i];
            err_row1[i] = err_row2[i];
            err_row2[i] = 0;
            i += 1;
        }
    }
}

#[inline]
fn atkinson_add(cell: &mut i16, delta: i16) {
    *cell = cell.saturating_add(delta).clamp(-72, 72);
}
