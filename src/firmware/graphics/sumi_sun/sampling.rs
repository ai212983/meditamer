use super::*;
use embedded_graphics::pixelcolor::BinaryColor;

mod noise_math;

use noise_math::{
    atan2_fast_fx, dist_px_fx, fx_i32, lerp, smoothstep01, smoothstep_edges, texture_noise01,
    value_noise01, wrap01,
};

pub(super) fn sample_sumi_sun_binary_pixel(
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
pub(super) fn sample_sumi_sun_gray4_level(
    x: i32,
    y: i32,
    params: SumiSunParams,
    dither: DitherMode,
) -> u8 {
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
