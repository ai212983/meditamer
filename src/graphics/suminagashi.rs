use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, OriginDimensions, Pixel, Point, Size},
};
use fixed::types::I16F16;
use fixed_sqrt::FixedSqrt;
use heapless::Vec;

include!("../assets/suminagashi_blue_noise.rs");

pub type Fx = I16F16;

const FX_ZERO: Fx = Fx::from_bits(0);
const FX_HALF: Fx = Fx::from_bits(1 << 15);
const FX_ONE: Fx = Fx::from_bits(1 << 16);
const FX_TWO: Fx = Fx::from_bits(2 << 16);
const FX_THREE: Fx = Fx::from_bits(3 << 16);
const FX_SIX: Fx = Fx::from_bits(6 << 16);
const FX_EIGHT: Fx = Fx::from_bits(8 << 16);
const FX_FIFTEEN: Fx = Fx::from_bits(15 << 16);
const FX_HUNDRED: Fx = Fx::from_bits(100 << 16);
const FX_PI: Fx = Fx::from_bits(205_887);
const FX_TAU: Fx = Fx::from_bits(411_775);
const FX_EPSILON: Fx = Fx::from_bits(16);

const FX_0_03: Fx = Fx::from_bits(1_966);
const FX_0_05: Fx = Fx::from_bits(3_277);
const FX_0_1: Fx = Fx::from_bits(6_554);
const FX_0_11: Fx = Fx::from_bits(7_209);
const FX_0_12: Fx = Fx::from_bits(7_864);
const FX_0_15: Fx = Fx::from_bits(9_830);
const FX_0_2: Fx = Fx::from_bits(13_107);
const FX_0_3: Fx = Fx::from_bits(19_661);
const FX_0_45: Fx = Fx::from_bits(29_491);
const FX_0_55: Fx = Fx::from_bits(36_045);
const FX_0_9: Fx = Fx::from_bits(58_982);
const FX_20: Fx = Fx::from_bits(1_310_720);
const FX_40: Fx = Fx::from_bits(2_621_440);
const FX_150: Fx = Fx::from_bits(9_830_400);

const FX_LUMA_R: Fx = Fx::from_bits(19_595);
const FX_LUMA_G: Fx = Fx::from_bits(38_470);
const FX_LUMA_B: Fx = Fx::from_bits(7_471);

const MAX_OPERATORS: usize = 64;

const DEFAULT_GEN_DENSITY: usize = 19;
const DEFAULT_GEN_ENTROPY: Fx = FX_0_2;
const DEFAULT_GEN_FLOW: Fx = FX_0_1;

const DEFAULT_GRAIN_SCALE: Fx = FX_THREE;
const DEFAULT_GRAIN_STRENGTH: Fx = FX_0_03;
const DEFAULT_PAPER_R: Fx = FX_ONE;
const DEFAULT_PAPER_G: Fx = FX_ONE;
const DEFAULT_PAPER_B: Fx = FX_ONE;

const DEFAULT_SUMI_EDGE: Fx = Fx::from_bits(20_972);
const DEFAULT_SUMI_FIBERS: Fx = Fx::from_bits(12_452);
const DEFAULT_SUMI_GRAIN: Fx = FX_HALF;
const DEFAULT_SUMI_RIM: Fx = FX_0_3;
const DEFAULT_INK_R: Fx = FX_0_12;
const DEFAULT_INK_G: Fx = FX_0_11;
const DEFAULT_INK_B: Fx = FX_0_1;
const SUMINAGASHI_ENABLE_PAPER_GRAIN: bool = false;
const SUMINAGASHI_ENABLE_INK_GRAIN: bool = true;

const NOISE_ROT_COS: Fx = Fx::from_bits(57_513); // cos(0.5)
const NOISE_ROT_SIN: Fx = Fx::from_bits(31_420); // sin(0.5)

const BAYER_4X4: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
const BLUE_NOISE_600_WIDTH: usize = 600;
const BLUE_NOISE_600_HEIGHT: usize = 600;
const BLUE_NOISE_600: &[u8; BLUE_NOISE_600_WIDTH * BLUE_NOISE_600_HEIGHT] =
    include_bytes!("../assets/suminagashi_blue_noise_600.bin");

#[derive(Clone, Copy, Debug, Default)]
pub struct Vec2Fx {
    pub x: Fx,
    pub y: Fx,
}

impl Vec2Fx {
    #[inline]
    pub const fn new(x: Fx, y: Fx) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn dot(self, other: Self) -> Fx {
        self.x * other.x + self.y * other.y
    }

    #[inline]
    pub fn norm2(self) -> Fx {
        self.dot(self)
    }

    #[inline]
    pub fn norm(self) -> Fx {
        let n2 = self.norm2();
        if n2 <= FX_ZERO {
            FX_ZERO
        } else {
            sqrt_fx(n2)
        }
    }
}

impl core::ops::Add for Vec2Fx {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl core::ops::Sub for Vec2Fx {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl core::ops::Mul<Fx> for Vec2Fx {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Fx) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl core::ops::Div<Fx> for Vec2Fx {
    type Output = Self;

    #[inline]
    fn div(self, rhs: Fx) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

#[derive(Clone, Copy, Debug)]
struct DropOperator {
    center: Vec2Fx, // normalized [0,1] in source space
    radius: Fx,     // normalized by height (shader units)
    ink_on: bool,
}

#[derive(Clone, Copy, Debug)]
struct SwirlOperator {
    center: Vec2Fx, // normalized [0,1]
    strength: Fx,   // radians
    radius: Fx,     // normalized by height
}

#[derive(Clone, Copy, Debug)]
struct FlowCombOperator {
    center: Vec2Fx, // normalized [0,1]
    dir: Vec2Fx,    // unit direction in shader space
    strength: Fx,
    radius: Fx,
}

#[derive(Clone, Copy, Debug)]
enum Operator {
    Drop(DropOperator),
    Swirl(SwirlOperator),
    FlowComb(FlowCombOperator),
}

#[derive(Clone, Copy, Debug)]
struct SumiInkParams {
    edge_softness: Fx,
    fiber_strength: Fx,
    ink_grain: Fx,
    rim_strength: Fx,
}

pub struct MarblingScene {
    width: i32,
    height: i32,
    aspect: Fx,
    paper_luma: Fx,
    ink_luma: Fx,
    grain_scale: Fx,
    grain_strength: Fx,
    sumi: SumiInkParams,
    ops: Vec<Operator, MAX_OPERATORS>,
}

impl MarblingScene {
    pub fn new(size: Size) -> Self {
        let width = size.width as i32;
        let height = size.height as i32;
        let safe_h = height.max(1);
        let aspect = fx_i32(width.max(1)) / fx_i32(safe_h);

        let paper_luma = luminance(DEFAULT_PAPER_R, DEFAULT_PAPER_G, DEFAULT_PAPER_B);
        let ink_luma = luminance(DEFAULT_INK_R, DEFAULT_INK_G, DEFAULT_INK_B);

        Self {
            width,
            height,
            aspect,
            paper_luma,
            ink_luma,
            grain_scale: DEFAULT_GRAIN_SCALE,
            grain_strength: if SUMINAGASHI_ENABLE_PAPER_GRAIN {
                DEFAULT_GRAIN_STRENGTH
            } else {
                FX_ZERO
            },
            sumi: SumiInkParams {
                edge_softness: DEFAULT_SUMI_EDGE,
                fiber_strength: DEFAULT_SUMI_FIBERS,
                ink_grain: if SUMINAGASHI_ENABLE_INK_GRAIN {
                    DEFAULT_SUMI_GRAIN
                } else {
                    FX_ZERO
                },
                rim_strength: DEFAULT_SUMI_RIM,
            },
            ops: Vec::new(),
        }
    }

    pub fn push_drop(&mut self, center: Vec2Fx, radius: Fx, ink_on: bool) -> bool {
        self.ops
            .push(Operator::Drop(DropOperator {
                center,
                radius: radius.max(FX_EPSILON),
                ink_on,
            }))
            .is_ok()
    }

    pub fn push_swirl(&mut self, center: Vec2Fx, strength: Fx, radius: Fx) -> bool {
        self.ops
            .push(Operator::Swirl(SwirlOperator {
                center,
                strength,
                radius: radius.max(FX_EPSILON),
            }))
            .is_ok()
    }

    pub fn push_flow_comb(
        &mut self,
        center: Vec2Fx,
        dir: Vec2Fx,
        strength: Fx,
        radius: Fx,
    ) -> bool {
        let norm = dir.norm();
        let dir = if norm <= FX_EPSILON {
            Vec2Fx::new(FX_ONE, FX_ZERO)
        } else {
            dir / norm
        };

        self.ops
            .push(Operator::FlowComb(FlowCombOperator {
                center,
                dir,
                strength,
                radius: radius.max(FX_EPSILON),
            }))
            .is_ok()
    }

    fn sample_inverse_luma(&self, st: Vec2Fx, mut p: Vec2Fx) -> Fx {
        let mut gray = self.paper_luma;

        for op in self.ops.iter().rev() {
            match *op {
                Operator::Drop(drop) => {
                    let center = Vec2Fx::new(drop.center.x * self.aspect, drop.center.y);
                    let v = p - center;
                    let dist_sq = v.norm2();
                    let r_sq = drop.radius * drop.radius;

                    if dist_sq < r_sq {
                        if drop.ink_on {
                            gray = shade_black_drop(self, p, center, drop.radius, dist_sq);
                        } else {
                            gray = self.paper_luma;
                        }
                        break;
                    }

                    if dist_sq > FX_EPSILON {
                        let inner = (FX_ONE - (r_sq / dist_sq)).max(FX_ZERO);
                        let factor = sqrt_fx(inner);
                        p = center + v * factor;
                    }
                }
                Operator::Swirl(swirl) => {
                    let center = Vec2Fx::new(swirl.center.x * self.aspect, swirl.center.y);
                    let v = p - center;
                    let dist = v.norm();
                    if dist < swirl.radius {
                        let pct = (swirl.radius - dist) / swirl.radius;
                        let angle = swirl.strength * pct * pct;
                        p = center + rotate(v, -angle);
                    }
                }
                Operator::FlowComb(comb) => {
                    let center = Vec2Fx::new(comb.center.x * self.aspect, comb.center.y);
                    let v = p - center;
                    let perp = Vec2Fx::new(-comb.dir.y, comb.dir.x);
                    let dist = v.dot(perp).abs();
                    if dist < comb.radius {
                        let pct = (comb.radius - dist) / comb.radius;
                        let disp = comb.strength * pct * pct;
                        p = p - comb.dir * disp;
                    }
                }
            }
        }

        // Keep untouched paper fully white. Grain is only applied to inked regions.
        if gray < self.paper_luma - FX_EPSILON {
            let grain = random01(st * self.grain_scale) * self.grain_strength;
            (gray - grain).clamp(FX_ZERO, FX_ONE)
        } else {
            self.paper_luma
        }
    }
}

#[derive(Clone, Copy)]
pub enum RgssMode {
    X1,
    X4,
    X8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Mono1,
    Gray4,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DitherMode {
    Bayer4x4,
    BlueNoise32,
    BlueNoise600,
}

#[derive(Clone, Copy)]
pub struct SceneRenderStyle {
    pub rgss: RgssMode,
    pub mode: RenderMode,
    pub dither: DitherMode,
}

impl RgssMode {
    #[inline]
    fn offsets(self) -> &'static [Vec2Fx] {
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
fn sample_binary_pixel(
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
fn sample_mono1_pixel(
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
fn sample_gray4_level(
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
fn sample_gray(scene: &MarblingScene, x: i32, y: i32, rgss: RgssMode) -> Fx {
    let offsets = rgss.offsets();
    let mut sum = FX_ZERO;

    for offset in offsets {
        let (st, p) = pixel_to_shader_space(scene, x, y, *offset);
        sum += scene.sample_inverse_luma(st, p);
    }

    sum / fx_i32(offsets.len() as i32)
}

#[inline]
fn pixel_to_shader_space(
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
fn shade_black_drop(
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
fn bayer_threshold_4x4_u8(x: i32, y: i32) -> u8 {
    BAYER_4X4[(((y as usize) & 0x03) << 2) | ((x as usize) & 0x03)]
}

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
pub(crate) fn dither_threshold_u8(x: i32, y: i32, mode: DitherMode) -> u8 {
    match mode {
        DitherMode::Bayer4x4 => bayer_threshold_4x4_u8(x, y) << 4,
        DitherMode::BlueNoise32 => blue_noise_threshold_u8(x, y),
        DitherMode::BlueNoise600 => blue_noise_600_threshold_u8(x, y),
    }
}

#[inline]
fn dither_threshold_fx(x: i32, y: i32, mode: DitherMode) -> Fx {
    Fx::from_bits((dither_threshold_u8(x, y, mode) as i32) << 8)
}

#[inline]
fn luminance(r: Fx, g: Fx, b: Fx) -> Fx {
    r * FX_LUMA_R + g * FX_LUMA_G + b * FX_LUMA_B
}

fn blue_noise_fbm(mut p: Vec2Fx) -> Fx {
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
fn blue_noise01(st: Vec2Fx) -> Fx {
    let x = st.x.floor().to_num::<i32>();
    let y = st.y.floor().to_num::<i32>();
    Fx::from_bits((blue_noise_threshold_u8(x, y) as i32) << 8)
}

#[inline]
fn random01(st: Vec2Fx) -> Fx {
    // Quantize coordinates before hashing to produce stable pseudo-random grain.
    let q = fx_i32(4096);
    let x = (st.x * q).to_num::<i32>();
    let y = (st.y * q).to_num::<i32>();
    random_grid01(x, y, 0x6E2F_28A5)
}

#[inline]
fn random_grid01(x: i32, y: i32, seed: u32) -> Fx {
    hash_to_unit01(hash_xy(x, y, seed))
}

#[inline]
fn hash_to_unit01(h: u32) -> Fx {
    Fx::from_bits((h >> 16) as i32)
}

#[inline]
fn hash_xy(x: i32, y: i32, seed: u32) -> u32 {
    let mut v = seed ^ (x as u32).wrapping_mul(0x27D4_EB2D) ^ (y as u32).wrapping_mul(0x1656_67B1);
    v ^= v >> 15;
    v = v.wrapping_mul(0x85EB_CA6B);
    v ^= v >> 13;
    v = v.wrapping_mul(0xC2B2_AE35);
    v ^= v >> 16;
    v
}

#[inline]
fn smoothstep01(t: Fx) -> Fx {
    let t = t.clamp(FX_ZERO, FX_ONE);
    t * t * (FX_THREE - FX_TWO * t)
}

#[inline]
fn smoothstep_edges(edge0: Fx, edge1: Fx, x: Fx) -> Fx {
    let delta = edge1 - edge0;
    if delta.abs() <= FX_EPSILON {
        return if x >= edge1 { FX_ONE } else { FX_ZERO };
    }
    let t = ((x - edge0) / delta).clamp(FX_ZERO, FX_ONE);
    smoothstep01(t)
}

#[inline]
fn lerp(a: Fx, b: Fx, t: Fx) -> Fx {
    a + (b - a) * t
}

#[inline]
fn rotate(v: Vec2Fx, angle: Fx) -> Vec2Fx {
    let (s, c) = sin_cos(angle);
    Vec2Fx::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

#[inline]
fn sin_cos(angle: Fx) -> (Fx, Fx) {
    // Fast polynomial approximation after wrapping to [-pi, pi].
    let x = wrap_pi(angle);
    let x2 = x * x;
    let sin = x * (FX_ONE - (x2 / FX_SIX));
    let cos = FX_ONE - (x2 / FX_TWO) + ((x2 * x2) / fx_i32(24));
    (sin, cos)
}

#[inline]
fn wrap_pi(mut angle: Fx) -> Fx {
    while angle > FX_PI {
        angle -= FX_TAU;
    }
    while angle < -FX_PI {
        angle += FX_TAU;
    }
    angle
}

#[inline]
fn sqrt_fx(v: Fx) -> Fx {
    FixedSqrt::sqrt(v.max(FX_ZERO))
}

#[inline]
const fn fx_i32(v: i32) -> Fx {
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
