use super::suminagashi::{dither_threshold_u8, DitherMode, RenderMode};
use embedded_graphics::prelude::Point;
use fixed::types::I16F16;

include!("../assets/suminagashi_blue_noise.rs");

mod render;
mod sampling;
pub use render::{render_sumi_sun, render_sumi_sun_gray4_packed, render_sumi_sun_rows_bw};

pub type Fx = I16F16;

const FX_ZERO: Fx = Fx::from_bits(0);
const FX_HALF: Fx = Fx::from_bits(1 << 15);
const FX_ONE: Fx = Fx::from_bits(1 << 16);
const FX_TWO: Fx = Fx::from_bits(2 << 16);
const FX_THREE: Fx = Fx::from_bits(3 << 16);
const FX_1_8: Fx = Fx::from_bits(117_965);
const FX_2_2: Fx = Fx::from_bits(144_179);
const FX_TAU: Fx = Fx::from_bits(411_775);
const FX_EPSILON: Fx = Fx::from_bits(16);
const FX_PI_OVER_4: Fx = Fx::from_bits(51_472);
const FX_3PI_OVER_4: Fx = Fx::from_bits(154_416);
const FX_0_05: Fx = Fx::from_bits(3_277);
const FX_0_12: Fx = Fx::from_bits(7_864);
const FX_0_2: Fx = Fx::from_bits(13_107);
const FX_0_45: Fx = Fx::from_bits(29_491);
const FX_0_55: Fx = Fx::from_bits(36_045);
const FX_255: Fx = Fx::from_bits(16_711_680);

const DEFAULT_SUN_EDGE_SOFTNESS_PX: Fx = Fx::from_bits(131_072); // 2.0 px
const DEFAULT_SUN_BLEED_PX: Fx = Fx::from_bits(360_448); // 5.5 px
const DEFAULT_SUN_DRY_BRUSH: Fx = FX_0_2;
const DEFAULT_SUN_COMPLETENESS: Fx = FX_ONE;
const DEFAULT_SUN_COMPLETENESS_SOFTNESS: Fx = FX_0_05;
const DEFAULT_SUN_COMPLETENESS_WARP: Fx = FX_0_12;
const DEFAULT_SUN_COMPLETENESS_ROTATION: Fx = Fx::from_bits(21_627); // ~0.33 turn
const DEFAULT_SUN_STROKE_STRENGTH: Fx = Fx::from_bits(11_796); // ~0.18
const DEFAULT_SUN_STROKE_ANISOTROPY: Fx = Fx::from_bits(229_376); // 3.5
const DEFAULT_SUN_INK_LUMA: Fx = Fx::from_bits(7_333); // ~0.112

const BLUE_NOISE_600_WIDTH: usize = 600;
const BLUE_NOISE_600_HEIGHT: usize = 600;
const BLUE_NOISE_600: &[u8; BLUE_NOISE_600_WIDTH * BLUE_NOISE_600_HEIGHT] =
    include_bytes!("../assets/suminagashi_blue_noise_600.bin");

#[derive(Clone, Copy, Debug)]
pub struct SumiSunParams {
    pub center: Point,
    pub radius_px: i32,
    pub edge_softness_px: Fx,
    pub bleed_px: Fx,
    pub dry_brush: Fx,
    pub completeness: Fx,
    pub completeness_softness: Fx,
    pub completeness_warp: Fx,
    pub completeness_rotation: Fx, // [0,1] turn
    pub stroke_strength: Fx,
    pub stroke_anisotropy: Fx,
    pub ink_luma: Fx,
}

impl Default for SumiSunParams {
    fn default() -> Self {
        Self {
            center: Point::new(300, 300),
            radius_px: 120,
            edge_softness_px: DEFAULT_SUN_EDGE_SOFTNESS_PX,
            bleed_px: DEFAULT_SUN_BLEED_PX,
            dry_brush: DEFAULT_SUN_DRY_BRUSH,
            completeness: DEFAULT_SUN_COMPLETENESS,
            completeness_softness: DEFAULT_SUN_COMPLETENESS_SOFTNESS,
            completeness_warp: DEFAULT_SUN_COMPLETENESS_WARP,
            completeness_rotation: DEFAULT_SUN_COMPLETENESS_ROTATION,
            stroke_strength: DEFAULT_SUN_STROKE_STRENGTH,
            stroke_anisotropy: DEFAULT_SUN_STROKE_ANISOTROPY,
            ink_luma: DEFAULT_SUN_INK_LUMA,
        }
    }
}
