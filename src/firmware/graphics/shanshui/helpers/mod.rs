use super::*;

mod noise;
pub(super) use noise::*;

pub(super) fn river_override(
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
    let far_c = layer_height(
        center,
        width,
        height,
        seed ^ 0x4A56_CE3D,
        7,
        FX_0_35,
        FX_0_35,
    );
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
pub(super) fn river_tree_exclusion(x: i32, y: i32, width: i32, height: i32, seed: u32) -> bool {
    let center = river_center_x(y, width, height, seed);
    let far_c = layer_height(
        center,
        width,
        height,
        seed ^ 0x4A56_CE3D,
        7,
        FX_0_35,
        FX_0_35,
    );
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

    let half_w =
        (fx_i32(river_half_width(y, width, height, valley_floor, depth_factor, seed).max(2))
            * (FX_0_35 + row_strength * FX_0_6))
            .to_num::<i32>()
            .max(3);
    (x - center).abs() <= half_w + 6
}

#[inline]
pub(super) fn river_center_x(y: i32, width: i32, height: i32, seed: u32) -> i32 {
    let river_start = (height * 42) / 100;
    let macro_n = value_noise1d(y + river_start, seed ^ 0xA4F7_2C19, 7) - FX_HALF;
    let micro_n = value_noise1d(y + 91, seed ^ 0x5D38_CEA3, 8) - FX_HALF;

    let macro_amp = fx_i32((width / 4).max(1));
    let micro_amp = fx_i32((width / 30).max(1));
    let offset = (macro_n * macro_amp + micro_n * micro_amp).to_num::<i32>();
    (width / 2 + offset).clamp(0, width - 1)
}

#[inline]
pub(super) fn river_half_width(
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
    let t =
        (depth_factor * Fx::from_bits(39_322) + y_t * Fx::from_bits(26_214)).clamp(FX_ZERO, FX_ONE);

    let top_w = 5;
    let bottom_w = (width / 6).clamp(28, 120);
    let base = fx_i32(top_w) + fx_i32(bottom_w - top_w) * smoothstep01(t);

    let wobble =
        (value_noise1d(y.saturating_mul(3) + 17, seed ^ 0xC82A_6F51, 5) - FX_HALF) * fx_i32(5);
    (base + wobble)
        .to_num::<i32>()
        .clamp(4, (width / 3).max(12))
}

#[inline]
pub(super) fn tree_ink(x: i32, y: i32, _width: i32, height: i32, near_h: i32, seed: u32) -> bool {
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
pub(super) fn tree_halo(x: i32, y: i32, _width: i32, height: i32, near_h: i32, seed: u32) -> bool {
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
    (11 * 11..=15 * 15).contains(&dist_sq)
}

#[inline]
pub(super) fn tree_shape(x: i32, height: i32, near_h: i32, seed: u32) -> Option<TreeShape> {
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
pub(super) fn slope_abs(x: i32, width: i32, height: i32, seed: u32, layer: u8) -> i32 {
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
pub(super) fn layer_height(
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
    let detail_noise = value_noise1d(
        nx * 2 + 17,
        seed ^ 0x1B87_359D,
        cell_shift.saturating_sub(1),
    );
    let n = (macro_noise * FX_0_6 + detail_noise * FX_0_4) - FX_HALF;

    let base = fx_i32(h) * base_ratio;
    let amp = fx_i32(h) * amp_ratio;
    let y = (base + amp * n).to_num::<i32>();
    y.clamp(0, h - 1)
}

#[inline]
pub(super) fn ridge_noise(x: i32, y: i32, seed: u32, shift: u8) -> Fx {
    let n = value_noise2d(x, y, seed, shift);
    let ridged = FX_ONE - (n * FX_TWO - FX_ONE).abs();
    (ridged * ridged).clamp(FX_ZERO, FX_ONE)
}
