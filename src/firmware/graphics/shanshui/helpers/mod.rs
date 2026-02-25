use super::*;

mod noise;
mod river;

pub(super) use noise::*;
pub(super) use river::*;

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
