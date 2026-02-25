use embedded_graphics::prelude::Size;
use fixed::types::I16F16;
use heapless::Vec;

include!("../assets/suminagashi_blue_noise.rs");

mod helpers;
mod render;

pub use helpers::build_seeded_scene;
pub(crate) use helpers::dither_threshold_u8;
use helpers::{
    fx_i32, luminance, random01, rotate, sample_binary_pixel, sample_gray4_level, shade_black_drop,
    sqrt_fx,
};
pub use render::{
    render_scene_rows_bw, render_scene_rows_bw_masked, render_seeded_gray4_packed,
    render_seeded_inverse_rgss, render_seeded_inverse_rgss_bw,
};

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
