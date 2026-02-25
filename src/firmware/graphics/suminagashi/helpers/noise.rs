use super::*;
use fixed_sqrt::FixedSqrt;

pub(super) fn blue_noise_fbm(mut p: Vec2Fx) -> Fx {
    let mut value = FX_ZERO;
    let mut amp = FX_HALF;

    for _ in 0..4 {
        value += amp * blue_noise01(p);
        let xr = p.x * NOISE_ROT_COS + p.y * NOISE_ROT_SIN;
        let yr = -p.x * NOISE_ROT_SIN + p.y * NOISE_ROT_COS;
        // Integer translations decorrelate octaves without extra hashing.
        p = Vec2Fx::new(xr * FX_TWO + FX_HUNDRED, yr * FX_TWO + FX_40);
        amp *= FX_HALF;
    }

    value.clamp(FX_ZERO, FX_ONE)
}

#[inline]
pub(super) fn blue_noise01(st: Vec2Fx) -> Fx {
    let x = st.x.floor().to_num::<i32>();
    let y = st.y.floor().to_num::<i32>();
    Fx::from_bits((blue_noise_threshold_u8(x, y) as i32) << 8)
}

#[inline]
pub(crate) fn random01(st: Vec2Fx) -> Fx {
    // Quantize coordinates before hashing to produce stable pseudo-random grain.
    let q = fx_i32(4096);
    let x = (st.x * q).to_num::<i32>();
    let y = (st.y * q).to_num::<i32>();
    random_grid01(x, y, 0x6E2F_28A5)
}

#[inline]
pub(super) fn random_grid01(x: i32, y: i32, seed: u32) -> Fx {
    hash_to_unit01(hash_xy(x, y, seed))
}

#[inline]
pub(super) fn hash_to_unit01(h: u32) -> Fx {
    Fx::from_bits((h >> 16) as i32)
}

#[inline]
pub(super) fn hash_xy(x: i32, y: i32, seed: u32) -> u32 {
    let mut v = seed ^ (x as u32).wrapping_mul(0x27D4_EB2D) ^ (y as u32).wrapping_mul(0x1656_67B1);
    v ^= v >> 15;
    v = v.wrapping_mul(0x85EB_CA6B);
    v ^= v >> 13;
    v = v.wrapping_mul(0xC2B2_AE35);
    v ^= v >> 16;
    v
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
pub(crate) fn rotate(v: Vec2Fx, angle: Fx) -> Vec2Fx {
    let (s, c) = sin_cos(angle);
    Vec2Fx::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

#[inline]
pub(super) fn sin_cos(angle: Fx) -> (Fx, Fx) {
    // Fast polynomial approximation after wrapping to [-pi, pi].
    let x = wrap_pi(angle);
    let x2 = x * x;
    let sin = x * (FX_ONE - (x2 / FX_SIX));
    let cos = FX_ONE - (x2 / FX_TWO) + ((x2 * x2) / fx_i32(24));
    (sin, cos)
}

#[inline]
pub(super) fn wrap_pi(mut angle: Fx) -> Fx {
    while angle > FX_PI {
        angle -= FX_TAU;
    }
    while angle < -FX_PI {
        angle += FX_TAU;
    }
    angle
}

#[inline]
pub(crate) fn sqrt_fx(v: Fx) -> Fx {
    FixedSqrt::sqrt(v.max(FX_ZERO))
}

#[inline]
pub(crate) const fn fx_i32(v: i32) -> Fx {
    Fx::from_bits(v << 16)
}

#[derive(Clone, Copy)]
pub(super) struct Mulberry32 {
    state: u32,
}

impl Mulberry32 {
    #[inline]
    pub(super) fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        let mut t = self.state.wrapping_add(0x6D2B_79F5);
        self.state = t;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }

    #[inline]
    pub(super) fn next_fx01(&mut self) -> Fx {
        Fx::from_bits((self.next_u32() >> 16) as i32)
    }
}
