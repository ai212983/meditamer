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
        4 + layer as u8,
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

#[inline]
fn river_override(
    base_ink: u8,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    seed: u32,
) -> u8 {
    let river_start = (height * 42) / 100;
    let center = river_center_x(y, width, height, seed);
    // Evaluate terrain depth on the river centerline (stable across x for this y)
    // to avoid vertical cut artifacts where local mountain depth changes abruptly.
    let far_c = layer_height(center, width, height, seed ^ 0x4A56_CE3D, 7, FX_0_35, FX_0_35);
    let mid_c = layer_height(
        center + 67,
        width,
        height,
        seed ^ 0x891E_2B6F,
        6,
        FX_0_45,
        FX_0_4,
    );
    let near_c = layer_height(
        center + 131,
        width,
        height,
        seed ^ 0x2D9F_7A43,
        5,
        FX_0_6,
        FX_0_45,
    );
    let depth_factor = depth_factor_from_surfaces(y, &[far_c, mid_c, near_c], height);
    let valley_floor = river_start + 18;
    let row_strength = river_row_strength(y, valley_floor);
    if row_strength <= FX_ZERO {
        return base_ink;
    }

    let half_w_base = river_half_width(y, width, height, valley_floor, depth_factor, seed);
    let half_w = (fx_i32(half_w_base.max(1)) * (FX_0_35 + row_strength * FX_0_6))
        .to_num::<i32>()
        .max(3);
    let dx = (x - center).abs();
    let signed = dx - half_w;
    let depth_bits = depth_factor.to_bits().max(0) as u32;
    let bank_ink = (108
        + ((depth_bits * 44) >> 16) as i32
        + (((row_strength.to_bits().max(0) as u32) * 22) >> 16) as i32)
        .clamp(0, 255) as u8;
    let water_noise = (hash_xy(x, y, seed ^ 0x8BF1_4D3C) >> 24) as u8;
    let water_ink = (4 + (((water_noise as u16) * 14) >> 8) as u8).min(22);

    // Keep a thin connected upstream thread so the river never breaks into islands.
    let thread_w = 1 + (((row_strength.to_bits().max(0) as u32) >> 16) as i32);
    if dx <= thread_w && row_strength < FX_0_45 {
        let thread_blend = (Fx::from_bits(9_830) + row_strength * FX_0_6).clamp(FX_ZERO, FX_ONE);
        return blend_u8(base_ink, water_ink.saturating_add(2), thread_blend);
    }

    let edge_soft = 3 + ((depth_bits * 2) >> 16) as i32;
    let bank_soft = edge_soft + 2;
    if signed > bank_soft {
        return base_ink;
    }

    if signed <= 0 {
        let inside = (-signed).min(edge_soft.max(1) * 2);
        let t = Fx::from_bits(((inside << 16) / edge_soft.max(1)).clamp(0, 65_536));
        let t = smoothstep01(t);
        let river_ink = blend_u8(bank_ink, water_ink, t);
        let blend = (FX_0_6 + row_strength * FX_0_4).clamp(FX_ZERO, FX_ONE);
        return blend_u8(base_ink, river_ink, blend);
    }

    let d = signed.min(bank_soft);
    let t = Fx::from_bits((((bank_soft - d) << 16) / bank_soft.max(1)).clamp(0, 65_536));
    let t = (smoothstep01(t) * row_strength).clamp(FX_ZERO, FX_ONE);
    blend_u8(base_ink, bank_ink, t)
}

#[inline]
fn river_tree_exclusion(x: i32, y: i32, width: i32, height: i32, seed: u32) -> bool {
    let center = river_center_x(y, width, height, seed);
    let far_c = layer_height(center, width, height, seed ^ 0x4A56_CE3D, 7, FX_0_35, FX_0_35);
    let mid_c = layer_height(
        center + 67,
        width,
        height,
        seed ^ 0x891E_2B6F,
        6,
        FX_0_45,
        FX_0_4,
    );
    let near_c = layer_height(
        center + 131,
        width,
        height,
        seed ^ 0x2D9F_7A43,
        5,
        FX_0_6,
        FX_0_45,
    );
    let depth_factor = depth_factor_from_surfaces(y, &[far_c, mid_c, near_c], height);
    let river_start = (height * 42) / 100;
    let valley_floor = river_start + 18;
    let row_strength = river_row_strength(y, valley_floor);
    if row_strength <= FX_ZERO {
        return false;
    }

    let half_w = (fx_i32(river_half_width(y, width, height, valley_floor, depth_factor, seed).max(2))
        * (FX_0_35 + row_strength * FX_0_6))
        .to_num::<i32>()
        .max(3);
    (x - center).abs() <= half_w + 6
}

#[inline]
fn river_center_x(y: i32, width: i32, height: i32, seed: u32) -> i32 {
    let river_start = (height * 42) / 100;
    let macro_n = value_noise1d(y + river_start, seed ^ 0xA4F7_2C19, 7) - FX_HALF;
    let micro_n = value_noise1d(y + 91, seed ^ 0x5D38_CEA3, 8) - FX_HALF;

    let macro_amp = fx_i32((width / 4).max(1));
    let micro_amp = fx_i32((width / 30).max(1));
    let offset = (macro_n * macro_amp + micro_n * micro_amp).to_num::<i32>();
    (width / 2 + offset).clamp(0, width - 1)
}

#[inline]
fn river_half_width(
    y: i32,
    width: i32,
    height: i32,
    valley_floor: i32,
    depth_factor: Fx,
    seed: u32,
) -> i32 {
    let span = (height - valley_floor).max(1);
    let y_rel = (y - valley_floor).clamp(0, span);
    let y_t = Fx::from_bits((y_rel << 16) / span).clamp(FX_ZERO, FX_ONE);
    let t = (depth_factor * Fx::from_bits(39_322) + y_t * Fx::from_bits(26_214)).clamp(FX_ZERO, FX_ONE);

    let top_w = 5;
    let bottom_w = (width / 6).clamp(28, 120);
    let base = fx_i32(top_w) + fx_i32(bottom_w - top_w) * smoothstep01(t);

    let wobble = (value_noise1d(y.saturating_mul(3) + 17, seed ^ 0xC82A_6F51, 5) - FX_HALF)
        * fx_i32(5);
    (base + wobble).to_num::<i32>().clamp(4, (width / 3).max(12))
}

#[inline]
fn tree_ink(x: i32, y: i32, _width: i32, height: i32, near_h: i32, seed: u32) -> bool {
    let Some(shape) = tree_shape(x, height, near_h, seed) else {
        return false;
    };

    if y >= shape.trunk_top && y <= shape.root_y && (x - shape.root_x).abs() <= shape.trunk_w {
        return true;
    }

    let dx = x - shape.crown_x;
    let dy = y - shape.crown_y;
    let dist_sq = dx * dx + dy * dy;
    if dist_sq > 14 * 14 {
        return false;
    }

    let dot = hash_xy(x + shape.chunk_left, y, seed ^ 0x61C8_8647) as u8;
    dot < 168
}

#[inline]
fn tree_halo(x: i32, y: i32, _width: i32, height: i32, near_h: i32, seed: u32) -> bool {
    let Some(shape) = tree_shape(x, height, near_h, seed) else {
        return false;
    };

    let in_trunk_band = y >= shape.trunk_top - 1
        && y <= shape.root_y + 1
        && (x - shape.root_x).abs() <= shape.trunk_w + 1;
    let in_trunk_core =
        y >= shape.trunk_top && y <= shape.root_y && (x - shape.root_x).abs() <= shape.trunk_w;
    if in_trunk_band && !in_trunk_core {
        return true;
    }

    let dx = x - shape.crown_x;
    let dy = y - shape.crown_y;
    let dist_sq = dx * dx + dy * dy;
    dist_sq <= 15 * 15 && dist_sq >= 11 * 11
}

#[inline]
fn tree_shape(x: i32, height: i32, near_h: i32, seed: u32) -> Option<TreeShape> {
    let chunk_w = 52i32;
    let chunk = (x.max(0) / chunk_w) as u32;
    let h = mix32(seed ^ chunk.wrapping_mul(0x85EB_CA6B) ^ 0xA16F_2A39);
    if (h & 0xFF) > 112 {
        return None;
    }

    let phase = ((mix32(seed ^ chunk.wrapping_mul(0xD1B5_4A35) ^ 0x7F31_6C0B) >> 28) as i32) - 8;
    let chunk_left = (chunk as i32) * chunk_w + phase;
    let root_x = chunk_left + (((h >> 8) % (chunk_w as u32)) as i32);
    let root_y = near_h + (((h >> 26) & 0x03) as i32) - 1;
    if root_y <= 0 || root_y >= height {
        return None;
    }

    let trunk_h = 20 + (((h >> 16) & 0x1F) as i32);
    let trunk_w = 2 + (((h >> 21) & 0x01) as i32);
    let trunk_top = (root_y - trunk_h).max(0);
    let crown_x = root_x + ((((h >> 24) & 0x07) as i32) - 3);
    let crown_y = (trunk_top + 2).clamp(0, height - 1);

    Some(TreeShape {
        root_x,
        root_y,
        trunk_top,
        trunk_w,
        crown_x,
        crown_y,
        chunk_left,
    })
}

#[inline]
fn slope_abs(x: i32, width: i32, height: i32, seed: u32, layer: u8) -> i32 {
    let (shift, base, amp, salt) = match layer {
        0 => (7, FX_0_35, FX_0_35, 0x4A56_CE3D),
        1 => (6, FX_0_45, FX_0_4, 0x891E_2B6F),
        _ => (5, FX_0_6, FX_0_45, 0x2D9F_7A43),
    };
    let h_l = layer_height(x - 2, width, height, seed ^ salt, shift, base, amp);
    let h_r = layer_height(x + 2, width, height, seed ^ salt, shift, base, amp);
    ((h_r - h_l).abs() + 1) / 2
}

#[inline]
fn layer_height(
    x: i32,
    width: i32,
    height: i32,
    seed: u32,
    cell_shift: u8,
    base_ratio: Fx,
    amp_ratio: Fx,
) -> i32 {
    let w = width.max(1);
    let h = height.max(1);
    let nx = wrap_x(x, w);
    let macro_noise = value_noise1d(nx, seed, cell_shift);
    let detail_noise = value_noise1d(nx * 2 + 17, seed ^ 0x1B87_359D, cell_shift.saturating_sub(1));
    let n = (macro_noise * FX_0_6 + detail_noise * FX_0_4) - FX_HALF;

    let base = fx_i32(h) * base_ratio;
    let amp = fx_i32(h) * amp_ratio;
    let y = (base + amp * n).to_num::<i32>();
    y.clamp(0, h - 1)
}

#[inline]
fn ridge_noise(x: i32, y: i32, seed: u32, shift: u8) -> Fx {
    let n = value_noise2d(x, y, seed, shift);
    let ridged = FX_ONE - (n * FX_TWO - FX_ONE).abs();
    (ridged * ridged).clamp(FX_ZERO, FX_ONE)
}

#[inline]
fn value_noise1d(x: i32, seed: u32, cell_shift: u8) -> Fx {
    let shift = cell_shift.min(15);
    let cell = x >> shift;
    let frac_mask = (1i32 << shift) - 1;
    let frac = x & frac_mask;
    let t = Fx::from_bits((frac << (16 - shift as i32)).max(0));
    let t = smoothstep01(t);

    let n0 = hash_unit_fx(cell as u32, seed);
    let n1 = hash_unit_fx((cell + 1) as u32, seed);
    lerp(n0, n1, t)
}

#[inline]
fn value_noise2d(x: i32, y: i32, seed: u32, cell_shift: u8) -> Fx {
    let shift = cell_shift.min(15);
    let ix = x >> shift;
    let iy = y >> shift;
    let fx = x & ((1i32 << shift) - 1);
    let fy = y & ((1i32 << shift) - 1);
    let tx = smoothstep01(Fx::from_bits((fx << (16 - shift as i32)).max(0)));
    let ty = smoothstep01(Fx::from_bits((fy << (16 - shift as i32)).max(0)));

    let n00 = hash_xy_unit_fx(ix, iy, seed);
    let n10 = hash_xy_unit_fx(ix + 1, iy, seed);
    let n01 = hash_xy_unit_fx(ix, iy + 1, seed);
    let n11 = hash_xy_unit_fx(ix + 1, iy + 1, seed);

    let nx0 = lerp(n00, n10, tx);
    let nx1 = lerp(n01, n11, tx);
    lerp(nx0, nx1, ty)
}

#[inline]
fn smoothstep01(t: Fx) -> Fx {
    let t = t.clamp(FX_ZERO, FX_ONE);
    t * t * (FX_THREE - FX_TWO * t)
}

#[inline]
fn river_row_strength(y: i32, valley_floor: i32) -> Fx {
    let preblend_rows = 40;
    let fade_rows = 124;
    let rise = y - (valley_floor - preblend_rows);
    if rise <= 0 {
        return FX_ZERO;
    }
    let t = Fx::from_bits(((rise.min(fade_rows) << 16) / fade_rows).max(0)).clamp(FX_ZERO, FX_ONE);
    smoothstep01(t)
}

#[inline]
fn depth_factor_from_surfaces(y: i32, surfaces: &[i32; 3], height: i32) -> Fx {
    let mut w_sum = FX_ZERO;
    let mut d_sum = FX_ZERO;
    let mut idx = 0usize;
    while idx < surfaces.len() {
        let pen = (fx_i32(y - surfaces[idx])).clamp(FX_ZERO, fx_i32(height.max(1)));
        let band = fx_i32(24 + (idx as i32) * 12);
        let w = smoothstep01((pen / band).clamp(FX_ZERO, FX_ONE));
        let d = Fx::from_bits(((idx as i32) * 65_536) / 2);
        w_sum += w;
        d_sum += w * d;
        idx += 1;
    }
    if w_sum <= FX_ZERO {
        FX_ZERO
    } else {
        (d_sum / w_sum).clamp(FX_ZERO, FX_ONE)
    }
}

#[inline]
fn lerp_u16(a: u16, b: u16, t: Fx) -> u16 {
    let t_bits = t.clamp(FX_ZERO, FX_ONE).to_bits().max(0) as u32;
    let inv = 65_536u32.saturating_sub(t_bits);
    (((a as u32) * inv + (b as u32) * t_bits + 32_768) >> 16) as u16
}

#[inline]
fn lerp(a: Fx, b: Fx, t: Fx) -> Fx {
    a + (b - a) * t
}

#[inline]
fn blend_u8(a: u8, b: u8, t: Fx) -> u8 {
    lerp_u16(a as u16, b as u16, t) as u8
}

#[inline]
fn hash_unit_fx(v: u32, seed: u32) -> Fx {
    let h = mix32(v ^ seed);
    Fx::from_bits((h >> 16) as i32)
}

#[inline]
fn hash_xy_unit_fx(x: i32, y: i32, seed: u32) -> Fx {
    Fx::from_bits((hash_xy(x, y, seed) >> 16) as i32)
}

#[inline]
fn hash_xy(x: i32, y: i32, seed: u32) -> u32 {
    let mut v = seed ^ (x as u32).wrapping_mul(0x27D4_EB2D) ^ (y as u32).wrapping_mul(0x1656_67B1);
    v ^= v >> 15;
    v = v.wrapping_mul(0x85EB_CA6B);
    v ^= v >> 13;
    v = v.wrapping_mul(0xC2B2_AE35);
    v ^ (v >> 16)
}

#[inline]
fn wrap_x(x: i32, width: i32) -> i32 {
    let w = width.max(1);
    let m = x % w;
    if m < 0 {
        m + w
    } else {
        m
    }
}

#[inline]
fn mix32(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x85EB_CA6B);
    v ^= v >> 13;
    v = v.wrapping_mul(0xC2B2_AE35);
    v ^ (v >> 16)
}

#[inline]
const fn fx_i32(v: i32) -> Fx {
    Fx::from_bits(v << 16)
}
