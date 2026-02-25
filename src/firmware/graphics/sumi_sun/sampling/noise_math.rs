use super::*;

#[inline]
fn blue_noise_threshold_u8(x: i32, y: i32) -> u8 {
    let tx = (x as usize) & (BLUE_NOISE_SIDE - 1);
    let ty = (y as usize) & (BLUE_NOISE_SIDE - 1);
    BLUE_NOISE_32X32[ty * BLUE_NOISE_SIDE + tx]
}

#[inline]
fn blue_noise_600_threshold_u8(x: i32, y: i32) -> u8 {
    let tx = (x as usize) % BLUE_NOISE_600_WIDTH;
    let ty = (y as usize) % BLUE_NOISE_600_HEIGHT;
    BLUE_NOISE_600[ty * BLUE_NOISE_600_WIDTH + tx]
}

#[inline]
pub(super) fn texture_noise01(x: i32, y: i32, dither: DitherMode) -> Fx {
    let n = match dither {
        DitherMode::BlueNoise600 => blue_noise_600_threshold_u8(x, y),
        DitherMode::BlueNoise32 | DitherMode::Bayer4x4 => blue_noise_threshold_u8(x, y),
    };
    Fx::from_bits((n as i32) << 8)
}

#[inline]
pub(super) fn value_noise01(x: i32, y: i32, cell_shift: i32, seed: u32) -> Fx {
    let shift = cell_shift.clamp(0, 15) as u32;
    let cell = (1u32 << shift) as i32;
    let ix = x.div_euclid(cell);
    let iy = y.div_euclid(cell);
    let fx_num = x.rem_euclid(cell);
    let fy_num = y.rem_euclid(cell);
    let tx = Fx::from_bits((((fx_num as i64) << 16) / (cell as i64)) as i32);
    let ty = Fx::from_bits((((fy_num as i64) << 16) / (cell as i64)) as i32);
    let sx = smoothstep01(tx);
    let sy = smoothstep01(ty);

    let n00 = hash_noise01(ix, iy, seed);
    let n10 = hash_noise01(ix + 1, iy, seed);
    let n01 = hash_noise01(ix, iy + 1, seed);
    let n11 = hash_noise01(ix + 1, iy + 1, seed);

    let nx0 = lerp(n00, n10, sx);
    let nx1 = lerp(n01, n11, sx);
    lerp(nx0, nx1, sy)
}

#[inline]
fn hash_noise01(x: i32, y: i32, seed: u32) -> Fx {
    let mut h = seed ^ (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    Fx::from_bits((h >> 16) as i32)
}

#[inline]
pub(super) fn smoothstep01(t: Fx) -> Fx {
    let t = t.clamp(FX_ZERO, FX_ONE);
    t * t * (FX_THREE - FX_TWO * t)
}

#[inline]
pub(super) fn smoothstep_edges(edge0: Fx, edge1: Fx, x: Fx) -> Fx {
    let delta = edge1 - edge0;
    if delta.abs() <= FX_EPSILON {
        return if x >= edge1 { FX_ONE } else { FX_ZERO };
    }
    let t = ((x - edge0) / delta).clamp(FX_ZERO, FX_ONE);
    smoothstep01(t)
}

#[inline]
pub(super) fn lerp(a: Fx, b: Fx, t: Fx) -> Fx {
    a + (b - a) * t
}

#[inline]
pub(super) fn wrap01(mut t: Fx) -> Fx {
    while t < FX_ZERO {
        t += FX_ONE;
    }
    while t >= FX_ONE {
        t -= FX_ONE;
    }
    t
}

#[inline]
pub(super) fn atan2_fast_fx(y: Fx, x: Fx) -> Fx {
    if x.abs() <= FX_EPSILON {
        if y > FX_ZERO {
            return FX_PI_OVER_4 * FX_TWO;
        }
        if y < FX_ZERO {
            return -(FX_PI_OVER_4 * FX_TWO);
        }
        return FX_ZERO;
    }

    let ay = y.abs() + FX_EPSILON;
    let angle = if x >= FX_ZERO {
        let r = (x - ay) / (x + ay);
        FX_PI_OVER_4 - FX_PI_OVER_4 * r
    } else {
        let r = (x + ay) / (ay - x);
        FX_3PI_OVER_4 - FX_PI_OVER_4 * r
    };

    if y < FX_ZERO {
        -angle
    } else {
        angle
    }
}

#[inline]
pub(super) const fn fx_i32(v: i32) -> Fx {
    Fx::from_bits(v << 16)
}

#[inline]
pub(super) fn dist_px_fx(dx: i32, dy: i32) -> Fx {
    let dx = dx as i64;
    let dy = dy as i64;
    let dist_sq = (dx * dx + dy * dy) as u64;
    let bits = isqrt_u64(dist_sq << 32) as i32;
    Fx::from_bits(bits)
}

#[inline]
fn isqrt_u64(mut n: u64) -> u64 {
    let mut res = 0u64;
    let mut bit = 1u64 << 62;

    while bit > n {
        bit >>= 2;
    }

    while bit != 0 {
        if n >= res + bit {
            n -= res + bit;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }

    res
}
