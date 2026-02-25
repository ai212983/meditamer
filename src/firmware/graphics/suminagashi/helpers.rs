use super::*;
use fixed_sqrt::FixedSqrt;

impl RgssMode {
    #[inline]
    pub(super) fn offsets(self) -> &'static [Vec2Fx] {
        match self {
            Self::X1 => &RGSS1_OFFSETS,
            Self::X4 => &RGSS4_OFFSETS,
            Self::X8 => &RGSS8_OFFSETS,
        }
    }
}

const RGSS1_OFFSETS: [Vec2Fx; 1] = [Vec2Fx::new(FX_HALF, FX_HALF)];

const RGSS4_OFFSETS: [Vec2Fx; 4] = [
    Vec2Fx::new(Fx::from_bits(8_192), Fx::from_bits(24_576)),
    Vec2Fx::new(Fx::from_bits(24_576), Fx::from_bits(57_344)),
    Vec2Fx::new(Fx::from_bits(40_960), Fx::from_bits(8_192)),
    Vec2Fx::new(Fx::from_bits(57_344), Fx::from_bits(40_960)),
];

const RGSS8_OFFSETS: [Vec2Fx; 8] = [
    Vec2Fx::new(Fx::from_bits(4_096), Fx::from_bits(36_864)),
    Vec2Fx::new(Fx::from_bits(12_288), Fx::from_bits(4_096)),
    Vec2Fx::new(Fx::from_bits(20_480), Fx::from_bits(53_248)),
    Vec2Fx::new(Fx::from_bits(28_672), Fx::from_bits(20_480)),
    Vec2Fx::new(Fx::from_bits(36_864), Fx::from_bits(61_440)),
    Vec2Fx::new(Fx::from_bits(45_056), Fx::from_bits(28_672)),
    Vec2Fx::new(Fx::from_bits(53_248), Fx::from_bits(45_056)),
    Vec2Fx::new(Fx::from_bits(61_440), Fx::from_bits(12_288)),
];

pub fn build_seeded_scene(seed: u32, size: Size) -> MarblingScene {
    let mut rng = Mulberry32::new(seed);
    let mut scene = MarblingScene::new(size);

    let mut ink_on = true;
    for _ in 0..DEFAULT_GEN_DENSITY.min(50) {
        let drift_x = (rng.next_fx01() - FX_HALF) * FX_0_3;
        let drift_y = (rng.next_fx01() - FX_HALF) * FX_0_3;
        let center = Vec2Fx::new(FX_HALF + drift_x, FX_HALF + drift_y);
        let radius = FX_0_05 + rng.next_fx01() * FX_0_1;
        let _ = scene.push_drop(center, radius, ink_on);
        ink_on = !ink_on;
    }

    let num_swirls = (DEFAULT_GEN_ENTROPY * FX_EIGHT).floor().to_num::<usize>();
    for _ in 0..num_swirls {
        let center = Vec2Fx::new(rng.next_fx01(), rng.next_fx01());
        let strength = (rng.next_fx01() - FX_HALF) * FX_40 * DEFAULT_GEN_ENTROPY;
        let radius = FX_0_2 + rng.next_fx01() * FX_0_3;
        let _ = scene.push_swirl(center, strength, radius);
    }

    if DEFAULT_GEN_FLOW > FX_0_1 {
        let vertical = rng.next_fx01() > FX_HALF;
        let dir = if vertical {
            Vec2Fx::new(FX_ZERO, FX_ONE)
        } else {
            Vec2Fx::new(FX_ONE, FX_ZERO)
        };
        let _ = scene.push_flow_comb(
            Vec2Fx::new(FX_HALF, FX_HALF),
            dir,
            DEFAULT_GEN_FLOW * FX_HALF,
            FX_0_05,
        );
    }

    scene
}

#[inline]
pub(super) fn sample_binary_pixel(
    scene: &MarblingScene,
    x: i32,
    y: i32,
    rgss: RgssMode,
    mode: RenderMode,
    dither: DitherMode,
) -> BinaryColor {
    match mode {
        RenderMode::Mono1 => sample_mono1_pixel(scene, x, y, rgss, dither),
        RenderMode::Gray4 => {
            let level = sample_gray4_level(scene, x, y, rgss, dither);
            let threshold = dither_threshold_u8(x, y, dither) >> 4;
            if level > threshold {
                BinaryColor::On
            } else {
                BinaryColor::Off
            }
        }
    }
}

#[inline]
pub(super) fn sample_mono1_pixel(
    scene: &MarblingScene,
    x: i32,
    y: i32,
    rgss: RgssMode,
    dither: DitherMode,
) -> BinaryColor {
    let gray = sample_gray(scene, x, y, rgss);
    if gray < dither_threshold_fx(x, y, dither) {
        BinaryColor::On
    } else {
        BinaryColor::Off
    }
}

#[inline]
pub(super) fn sample_gray4_level(
    scene: &MarblingScene,
    x: i32,
    y: i32,
    rgss: RgssMode,
    dither: DitherMode,
) -> u8 {
    let gray = sample_gray(scene, x, y, rgss).clamp(FX_ZERO, FX_ONE);
    let threshold = dither_threshold_fx(x, y, dither);
    let level = (gray * FX_FIFTEEN + threshold).floor().to_num::<i32>();
    level.clamp(0, 15) as u8
}

#[inline]
pub(super) fn sample_gray(scene: &MarblingScene, x: i32, y: i32, rgss: RgssMode) -> Fx {
    let offsets = rgss.offsets();
    let mut sum = FX_ZERO;

    for offset in offsets {
        let (st, p) = pixel_to_shader_space(scene, x, y, *offset);
        sum += scene.sample_inverse_luma(st, p);
    }

    sum / fx_i32(offsets.len() as i32)
}

#[inline]
pub(super) fn pixel_to_shader_space(
    scene: &MarblingScene,
    x: i32,
    y: i32,
    offset: Vec2Fx,
) -> (Vec2Fx, Vec2Fx) {
    let width = fx_i32(scene.width.max(1));
    let height = fx_i32(scene.height.max(1));
    let st = Vec2Fx::new(
        (fx_i32(x) + offset.x) / width,
        (fx_i32(y) + offset.y) / height,
    );
    let p = Vec2Fx::new(st.x * scene.aspect, st.y);
    (st, p)
}

#[inline]
pub(super) fn shade_black_drop(
    scene: &MarblingScene,
    p: Vec2Fx,
    _center: Vec2Fx,
    radius: Fx,
    dist_sq: Fx,
) -> Fx {
    let dist = sqrt_fx(dist_sq.max(FX_ZERO));
    let norm_dist = dist / radius.max(FX_EPSILON);

    // Use precomputed blue-noise layers to keep texture organic while reducing per-pixel math.
    let fibers = blue_noise_fbm(p * FX_20);
    let perturbed_dist = norm_dist + scene.sumi.fiber_strength * (fibers - FX_HALF);

    let alpha = smoothstep_edges(FX_ONE, FX_ONE - scene.sumi.edge_softness, perturbed_dist);

    let cloud_density = blue_noise_fbm(p * FX_SIX);
    let grain = blue_noise01(p * FX_150);
    let mut density = lerp(cloud_density, grain, scene.sumi.ink_grain);

    let rim = smoothstep_edges(FX_ZERO, FX_0_15, perturbed_dist)
        * smoothstep_edges(FX_ONE, FX_0_9, perturbed_dist);
    density += scene.sumi.rim_strength * rim;
    density = (FX_0_45 + FX_0_55 * density).clamp(FX_ZERO, FX_ONE);

    lerp(scene.paper_luma, scene.ink_luma, alpha * density)
}

#[inline]
pub(super) fn bayer_threshold_4x4_u8(x: i32, y: i32) -> u8 {
    BAYER_4X4[(((y as usize) & 0x03) << 2) | ((x as usize) & 0x03)]
}

#[inline]
pub(super) fn blue_noise_threshold_u8(x: i32, y: i32) -> u8 {
    let tx = (x as usize) & (BLUE_NOISE_SIDE - 1);
    let ty = (y as usize) & (BLUE_NOISE_SIDE - 1);
    BLUE_NOISE_32X32[ty * BLUE_NOISE_SIDE + tx]
}

#[inline]
pub(super) fn blue_noise_600_threshold_u8(x: i32, y: i32) -> u8 {
    let tx = (x as usize) % BLUE_NOISE_600_WIDTH;
    let ty = (y as usize) % BLUE_NOISE_600_HEIGHT;
    BLUE_NOISE_600[ty * BLUE_NOISE_600_WIDTH + tx]
}

#[inline]
pub(crate) fn dither_threshold_u8(x: i32, y: i32, mode: DitherMode) -> u8 {
    match mode {
        DitherMode::Bayer4x4 => bayer_threshold_4x4_u8(x, y) << 4,
        DitherMode::BlueNoise32 => blue_noise_threshold_u8(x, y),
        DitherMode::BlueNoise600 => blue_noise_600_threshold_u8(x, y),
    }
}

#[inline]
pub(super) fn dither_threshold_fx(x: i32, y: i32, mode: DitherMode) -> Fx {
    Fx::from_bits((dither_threshold_u8(x, y, mode) as i32) << 8)
}

#[inline]
pub(super) fn luminance(r: Fx, g: Fx, b: Fx) -> Fx {
    r * FX_LUMA_R + g * FX_LUMA_G + b * FX_LUMA_B
}

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
pub(super) fn random01(st: Vec2Fx) -> Fx {
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
pub(super) fn rotate(v: Vec2Fx, angle: Fx) -> Vec2Fx {
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
pub(super) fn sqrt_fx(v: Fx) -> Fx {
    FixedSqrt::sqrt(v.max(FX_ZERO))
}

#[inline]
pub(super) const fn fx_i32(v: i32) -> Fx {
    Fx::from_bits(v << 16)
}

#[derive(Clone, Copy)]
struct Mulberry32 {
    state: u32,
}

impl Mulberry32 {
    #[inline]
    fn new(seed: u32) -> Self {
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
    fn next_fx01(&mut self) -> Fx {
        Fx::from_bits((self.next_u32() >> 16) as i32)
    }
}
