use super::suminagashi::{dither_threshold_u8, DitherMode, RenderMode};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::Point};
use fixed::types::I16F16;

include!("../assets/suminagashi_blue_noise.rs");

mod render;
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

fn sample_sumi_sun_binary_pixel(
    x: i32,
    y: i32,
    params: SumiSunParams,
    mode: RenderMode,
    dither: DitherMode,
) -> BinaryColor {
    match mode {
        RenderMode::Mono1 => {
            let gray_u8 = sample_sumi_sun_gray_u8(x, y, params, dither);
            if gray_u8 <= dither_threshold_u8(x, y, dither) {
                BinaryColor::On
            } else {
                BinaryColor::Off
            }
        }
        RenderMode::Gray4 => {
            let level = sample_sumi_sun_gray4_level(x, y, params, dither);
            let threshold = dither_threshold_u8(x, y, dither) >> 4;
            if level <= threshold {
                BinaryColor::On
            } else {
                BinaryColor::Off
            }
        }
    }
}

#[inline]
fn sample_sumi_sun_gray4_level(x: i32, y: i32, params: SumiSunParams, dither: DitherMode) -> u8 {
    let gray_u8 = sample_sumi_sun_gray_u8(x, y, params, dither) as i32;
    let n = dither_threshold_u8(x, y, dither) as i32 - 128;
    let dithered = gray_u8 + (n >> 4);
    ((dithered + 8) >> 4).clamp(0, 15) as u8
}

#[inline]
fn sample_sumi_sun_gray_u8(x: i32, y: i32, params: SumiSunParams, dither: DitherMode) -> u8 {
    let dx_px = x - params.center.x;
    let dy_px = y - params.center.y;
    let hard_clip_r = params.radius_px.max(1) + 2;
    let dist_sq_px = (dx_px as i64) * (dx_px as i64) + (dy_px as i64) * (dy_px as i64);
    let hard_clip_sq = (hard_clip_r as i64) * (hard_clip_r as i64);
    if dist_sq_px > hard_clip_sq {
        return 255;
    }
    let dx = fx_i32(dx_px);
    let dy = fx_i32(dy_px);
    let radius = fx_i32(params.radius_px.max(1));
    // Use integer distance math in pixel space to avoid I16F16 overflow at screen-scale values.
    let dist = dist_px_fx(dx_px, dy_px);

    let early_out = radius + params.bleed_px.max(FX_ZERO) + params.edge_softness_px.max(FX_ONE);
    if dist > early_out {
        return 255;
    }

    // Base signed distance to a disk, then perturbed with low-frequency noise for bleed.
    let low_noise = value_noise01(x, y, 2, 0x91A4_2E73);
    let bleed = params.bleed_px.max(FX_ZERO) * smoothstep01(low_noise);
    let sdf = dist - radius - bleed;

    let edge_soft = params.edge_softness_px.max(FX_EPSILON);
    let coverage = smoothstep_edges(edge_soft, -edge_soft, sdf);
    if coverage <= FX_EPSILON {
        return 255;
    }

    // Keep body visible but avoid collapsing to a solid black dot.
    let radial = (FX_ONE - (dist / radius)).clamp(FX_ZERO, FX_ONE);
    let mut density = (FX_0_45 + FX_0_55 * radial).clamp(FX_ZERO, FX_ONE);
    let rim_band = (edge_soft * FX_TWO).max(FX_EPSILON);
    let rim = (FX_ONE - (sdf.abs() / rim_band)).clamp(FX_ZERO, FX_ONE);
    density = (density + FX_0_05 * rim).clamp(FX_ZERO, FX_ONE);

    // Macro-contrast preset: favor large tonal structures over tiny speckles.
    let stretch = params.stroke_anisotropy.max(FX_ONE);
    let stretch_int = (stretch.to_bits() >> 16).max(1) as i64;
    let stretch_bits = stretch.to_bits().max(1) as i64;
    let sx = x.saturating_add((((y as i64) * stretch_int) >> 1) as i32);
    let sy = (((y as i64) * stretch_bits) >> 16).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    let stroke_noise = texture_noise01(sx, sy, dither);
    density = (density + (params.stroke_strength * FX_0_45) * (stroke_noise - FX_HALF))
        .clamp(FX_ZERO, FX_ONE);

    let lobe_noise_a = value_noise01(x, y, 5, 0x7A21_4C3D);
    let lobe_noise_b = value_noise01(
        x.saturating_add(y >> 2),
        y.saturating_sub(x >> 3),
        6,
        0x3D95_1A7B,
    );
    let lobe_noise = (lobe_noise_a + lobe_noise_b) * FX_HALF;
    density = (density + (params.stroke_strength * FX_THREE) * (lobe_noise - FX_HALF))
        .clamp(FX_ZERO, FX_ONE);

    let macro_noise = value_noise01(
        x.saturating_sub(y >> 3),
        y.saturating_add(x >> 4),
        6,
        0xC1B7_5D29,
    );
    density = (density * (FX_0_2 + FX_0_55 + FX_0_45 * macro_noise)).clamp(FX_ZERO, FX_ONE);
    density = ((density - FX_HALF) * FX_2_2 + FX_HALF).clamp(FX_ZERO, FX_ONE);

    // Angular completeness mask (optionally keeps part of the disk as void/mist).
    let mut completeness = params.completeness.clamp(FX_ZERO, FX_ONE);
    if completeness >= FX_ONE - FX_EPSILON {
        completeness = FX_ONE;
    }
    let fade_mask = if completeness <= FX_EPSILON {
        FX_ZERO
    } else if completeness >= FX_ONE {
        FX_ONE
    } else {
        let mut theta = atan2_fast_fx(dy, dx) + params.completeness_rotation * FX_TAU;
        if theta < FX_ZERO {
            theta += FX_TAU;
        }
        let theta01 = (theta / FX_TAU).clamp(FX_ZERO, FX_ONE);
        let warp =
            (value_noise01(x + 67, y + 19, 3, 0xA54B_9C0D) - FX_HALF) * params.completeness_warp;
        let t = wrap01(theta01 + warp);
        FX_ONE
            - smoothstep_edges(
                completeness - params.completeness_softness,
                completeness + params.completeness_softness,
                t,
            )
    };

    // Dry brush only near edge, carving tiny voids into the wash.
    let edge_band = (params.edge_softness_px * FX_THREE).max(FX_ONE);
    let edge_mask = (FX_ONE - (sdf.abs() / edge_band)).clamp(FX_ZERO, FX_ONE);
    let dry_noise = texture_noise01((x << 1) + 11, (y << 1) + 7, dither);
    let dry_mask = (FX_0_45 + FX_0_55 * edge_mask).clamp(FX_ZERO, FX_ONE);
    let dry_void = dry_mask * params.dry_brush * dry_noise * FX_HALF;
    let highlight_noise = value_noise01(
        x.saturating_add(y >> 3),
        y.saturating_add(x >> 3),
        5,
        0x2D8F_9E11,
    );
    let highlight_mask = smoothstep_edges(FX_0_45, FX_ONE, highlight_noise);
    let highlight_void = highlight_mask * (params.dry_brush * FX_TWO) * FX_0_55;

    let mut ink_alpha =
        (coverage * fade_mask * density - dry_void - highlight_void).clamp(FX_ZERO, FX_ONE);
    ink_alpha = ((ink_alpha - FX_HALF) * FX_1_8 + FX_HALF).clamp(FX_ZERO, FX_ONE);
    let ink_luma = params.ink_luma.clamp(FX_ZERO, FX_ONE);
    let gray = lerp(FX_ONE, ink_luma, ink_alpha).clamp(FX_ZERO, FX_ONE);
    ((gray * FX_255).to_bits() >> 16).clamp(0, 255) as u8
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
fn texture_noise01(x: i32, y: i32, dither: DitherMode) -> Fx {
    let n = match dither {
        DitherMode::BlueNoise600 => blue_noise_600_threshold_u8(x, y),
        DitherMode::BlueNoise32 | DitherMode::Bayer4x4 => blue_noise_threshold_u8(x, y),
    };
    Fx::from_bits((n as i32) << 8)
}

#[inline]
fn value_noise01(x: i32, y: i32, cell_shift: i32, seed: u32) -> Fx {
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
fn wrap01(mut t: Fx) -> Fx {
    while t < FX_ZERO {
        t += FX_ONE;
    }
    while t >= FX_ONE {
        t -= FX_ONE;
    }
    t
}

#[inline]
fn atan2_fast_fx(y: Fx, x: Fx) -> Fx {
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
const fn fx_i32(v: i32) -> Fx {
    Fx::from_bits(v << 16)
}

#[inline]
fn dist_px_fx(dx: i32, dy: i32) -> Fx {
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
