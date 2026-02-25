use super::*;

#[inline]
pub(in super::super) fn value_noise1d(x: i32, seed: u32, cell_shift: u8) -> Fx {
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
pub(in super::super) fn value_noise2d(x: i32, y: i32, seed: u32, cell_shift: u8) -> Fx {
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
pub(in super::super) fn smoothstep01(t: Fx) -> Fx {
    let t = t.clamp(FX_ZERO, FX_ONE);
    t * t * (FX_THREE - FX_TWO * t)
}

#[inline]
pub(in super::super) fn river_row_strength(y: i32, valley_floor: i32) -> Fx {
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
pub(in super::super) fn depth_factor_from_surfaces(y: i32, surfaces: &[i32; 3], height: i32) -> Fx {
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
pub(in super::super) fn lerp_u16(a: u16, b: u16, t: Fx) -> u16 {
    let t_bits = t.clamp(FX_ZERO, FX_ONE).to_bits().max(0) as u32;
    let inv = 65_536u32.saturating_sub(t_bits);
    (((a as u32) * inv + (b as u32) * t_bits + 32_768) >> 16) as u16
}

#[inline]
pub(in super::super) fn lerp(a: Fx, b: Fx, t: Fx) -> Fx {
    a + (b - a) * t
}

#[inline]
pub(in super::super) fn blend_u8(a: u8, b: u8, t: Fx) -> u8 {
    lerp_u16(a as u16, b as u16, t) as u8
}

#[inline]
pub(in super::super) fn hash_unit_fx(v: u32, seed: u32) -> Fx {
    let h = mix32(v ^ seed);
    Fx::from_bits((h >> 16) as i32)
}

#[inline]
pub(in super::super) fn hash_xy_unit_fx(x: i32, y: i32, seed: u32) -> Fx {
    Fx::from_bits((hash_xy(x, y, seed) >> 16) as i32)
}

#[inline]
pub(in super::super) fn hash_xy(x: i32, y: i32, seed: u32) -> u32 {
    let mut v = seed ^ (x as u32).wrapping_mul(0x27D4_EB2D) ^ (y as u32).wrapping_mul(0x1656_67B1);
    v ^= v >> 15;
    v = v.wrapping_mul(0x85EB_CA6B);
    v ^= v >> 13;
    v = v.wrapping_mul(0xC2B2_AE35);
    v ^ (v >> 16)
}

#[inline]
pub(in super::super) fn wrap_x(x: i32, width: i32) -> i32 {
    let w = width.max(1);
    let m = x % w;
    if m < 0 {
        m + w
    } else {
        m
    }
}

#[inline]
pub(in super::super) fn mix32(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x85EB_CA6B);
    v ^= v >> 13;
    v = v.wrapping_mul(0xC2B2_AE35);
    v ^ (v >> 16)
}

#[inline]
pub(in super::super) const fn fx_i32(v: i32) -> Fx {
    Fx::from_bits(v << 16)
}
