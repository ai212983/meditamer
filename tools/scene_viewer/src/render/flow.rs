use std::fs;

use crate::{
    bundle::{load_bundle, Bundle},
    cli::{mode_name, Config},
    io::{load_grayscale_resize, save_gray},
};

use super::{
    build_depth_relit_map, build_tone_lut, clamp_i16_to_u8, get_channel_or_default,
    ink_brush_delta, mix_u8, mul8, paper_noise_u8, quantize_u8, CH_ALBEDO, CH_AO, CH_DEPTH,
    CH_EDGE, CH_LIGHT, CH_MASK, CH_NORMAL_X, CH_NORMAL_Y, CH_STROKE,
};

struct RenderInputs<'a> {
    albedo: &'a [u8],
    light: &'a [u8],
    ao: &'a [u8],
    depth: &'a [u8],
    edge: &'a [u8],
    mask: &'a [u8],
    stroke: &'a [u8],
    normal_xy: Option<(&'a [u8], &'a [u8])>,
}

struct OptionalMaps {
    sun_light: Option<Vec<u8>>,
    ghost_prev: Option<Vec<u8>>,
}

struct RenderBuffers {
    tone_base: Vec<u8>,
    stylized: Vec<u8>,
    quantized: Vec<u8>,
}

pub(super) fn run_render_with_config(cfg: Config) -> Result<(), String> {
    let bundle = load_bundle(&cfg.bundle)?;
    let width = bundle.width as usize;
    let height = bundle.height as usize;
    let total = width * height;
    let inputs = collect_input_channels(&bundle, total)?;
    dump_channels_if_requested(&cfg, &bundle, &inputs)?;
    let optional = prepare_optional_maps(&cfg, &bundle, &inputs, width, height)?;
    let buffers = render_frame(&cfg, width, height, &inputs, &optional);
    save_render_outputs(&cfg, &bundle, &buffers, &optional)?;

    println!(
        "render mode={} levels={} dither={:?} edge_strength={} fog_strength={} stroke_strength={} paper_strength={} tone_curve={:?} sun_strength={} sun_azimuth_deg={} sun_elevation_deg={}",
        mode_name(cfg.mode),
        cfg.mode.levels(),
        cfg.dither,
        cfg.edge_strength,
        cfg.fog_strength,
        cfg.stroke_strength,
        cfg.paper_strength,
        cfg.tone_curve,
        cfg.sun_strength,
        cfg.sun_azimuth_deg,
        cfg.sun_elevation_deg
    );

    Ok(())
}

fn collect_input_channels<'a>(
    bundle: &'a Bundle,
    total: usize,
) -> Result<RenderInputs<'a>, String> {
    let albedo = get_channel_or_default(&bundle.channels, CH_ALBEDO, total, 255)?;
    let light = get_channel_or_default(&bundle.channels, CH_LIGHT, total, 255)?;
    let ao = get_channel_or_default(&bundle.channels, CH_AO, total, 255)?;
    let depth = get_channel_or_default(&bundle.channels, CH_DEPTH, total, 0)?;
    let edge = get_channel_or_default(&bundle.channels, CH_EDGE, total, 0)?;
    let mask = get_channel_or_default(&bundle.channels, CH_MASK, total, 255)?;
    let stroke = get_channel_or_default(&bundle.channels, CH_STROKE, total, 128)?;
    let normal_xy = collect_normal_xy(bundle, total);

    Ok(RenderInputs {
        albedo,
        light,
        ao,
        depth,
        edge,
        mask,
        stroke,
        normal_xy,
    })
}

fn collect_normal_xy<'a>(bundle: &'a Bundle, total: usize) -> Option<(&'a [u8], &'a [u8])> {
    let nx = bundle.channels.get(&CH_NORMAL_X)?;
    let ny = bundle.channels.get(&CH_NORMAL_Y)?;
    if nx.len() != total || ny.len() != total {
        return None;
    }
    let has_detail = nx
        .iter()
        .zip(ny.iter())
        .any(|(&x, &y)| x != 128 || y != 128);
    if has_detail {
        Some((nx.as_slice(), ny.as_slice()))
    } else {
        None
    }
}

fn dump_channels_if_requested(
    cfg: &Config,
    bundle: &Bundle,
    inputs: &RenderInputs<'_>,
) -> Result<(), String> {
    let Some(out_dir) = cfg.dump_channels.as_ref() else {
        return Ok(());
    };

    fs::create_dir_all(out_dir)
        .map_err(|e| format!("create dump channels dir {}: {e}", out_dir.display()))?;
    save_gray(
        &out_dir.join("albedo.png"),
        bundle.width,
        bundle.height,
        inputs.albedo,
    )?;
    save_gray(
        &out_dir.join("light.png"),
        bundle.width,
        bundle.height,
        inputs.light,
    )?;
    save_gray(
        &out_dir.join("ao.png"),
        bundle.width,
        bundle.height,
        inputs.ao,
    )?;
    save_gray(
        &out_dir.join("depth.png"),
        bundle.width,
        bundle.height,
        inputs.depth,
    )?;
    save_gray(
        &out_dir.join("edge.png"),
        bundle.width,
        bundle.height,
        inputs.edge,
    )?;
    save_gray(
        &out_dir.join("mask.png"),
        bundle.width,
        bundle.height,
        inputs.mask,
    )?;
    save_gray(
        &out_dir.join("stroke.png"),
        bundle.width,
        bundle.height,
        inputs.stroke,
    )?;
    if let Some((nx, ny)) = inputs.normal_xy {
        save_gray(
            &out_dir.join("normal_x.png"),
            bundle.width,
            bundle.height,
            nx,
        )?;
        save_gray(
            &out_dir.join("normal_y.png"),
            bundle.width,
            bundle.height,
            ny,
        )?;
    }
    Ok(())
}

fn prepare_optional_maps(
    cfg: &Config,
    bundle: &Bundle,
    inputs: &RenderInputs<'_>,
    width: usize,
    height: usize,
) -> Result<OptionalMaps, String> {
    let sun_light = if cfg.sun_strength > 0 {
        Some(build_depth_relit_map(
            inputs.depth,
            inputs.normal_xy,
            width,
            height,
            cfg.sun_azimuth_deg,
            cfg.sun_elevation_deg,
        ))
    } else {
        None
    };

    let ghost_prev = if let Some(path) = cfg.ghost_from.as_ref() {
        Some(load_grayscale_resize(path, bundle.width, bundle.height)?)
    } else {
        None
    };

    Ok(OptionalMaps {
        sun_light,
        ghost_prev,
    })
}

fn render_frame(
    cfg: &Config,
    width: usize,
    height: usize,
    inputs: &RenderInputs<'_>,
    optional: &OptionalMaps,
) -> RenderBuffers {
    let total = width * height;
    let tone_lut = build_tone_lut(cfg.tone_curve);
    let mut tone_base = vec![0u8; total];
    let mut stylized = vec![0u8; total];
    let mut quantized = vec![0u8; total];

    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let (base, stylized_px, quantized_px) =
                render_pixel(cfg, x, y, i, inputs, optional, &tone_lut);
            tone_base[i] = base;
            stylized[i] = stylized_px;
            quantized[i] = quantized_px;
        }
    }

    RenderBuffers {
        tone_base,
        stylized,
        quantized,
    }
}

fn render_pixel(
    cfg: &Config,
    x: usize,
    y: usize,
    i: usize,
    inputs: &RenderInputs<'_>,
    optional: &OptionalMaps,
    tone_lut: &[u8; 256],
) -> (u8, u8, u8) {
    let light_shaded = if let Some(sun_map) = optional.sun_light.as_ref() {
        mix_u8(inputs.light[i], sun_map[i], cfg.sun_strength)
    } else {
        inputs.light[i]
    };
    let base = mul8(mul8(inputs.albedo[i], light_shaded), inputs.ao[i]);

    let fog = mul8(inputs.depth[i], cfg.fog_strength);
    let fogged = mix_u8(base, 255, fog);

    let dark = mul8(inputs.edge[i], cfg.edge_strength);
    let edged = fogged.saturating_sub(dark);

    let stroke_delta = ink_brush_delta(
        i,
        x,
        y,
        inputs.stroke[i],
        inputs.edge[i],
        inputs.depth[i],
        inputs.normal_xy,
        cfg.stroke_strength,
    );
    let stroked = clamp_i16_to_u8((edged as i16) + stroke_delta);

    let paper_delta =
        ((paper_noise_u8(x as i32, y as i32) as i16) - 128) * (cfg.paper_strength as i16) / 255;
    let papered = clamp_i16_to_u8((stroked as i16) + paper_delta);
    let curved = tone_lut[papered as usize];
    let masked = mix_u8(255, curved, inputs.mask[i]);

    let stylized = if let Some(prev) = optional.ghost_prev.as_ref() {
        mix_u8(masked, prev[i], cfg.ghost_alpha)
    } else {
        masked
    };
    let quantized = quantize_u8(stylized, x as i32, y as i32, cfg.mode, cfg.dither);
    (base, stylized, quantized)
}

fn save_render_outputs(
    cfg: &Config,
    bundle: &Bundle,
    buffers: &RenderBuffers,
    optional: &OptionalMaps,
) -> Result<(), String> {
    if let Some(parent) = cfg.out.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create output dir {}: {e}", parent.display()))?;
    }
    save_gray(&cfg.out, bundle.width, bundle.height, &buffers.quantized)?;
    println!("wrote {}", cfg.out.display());

    if let Some(debug_dir) = cfg.save_debug.as_ref() {
        fs::create_dir_all(debug_dir)
            .map_err(|e| format!("create debug dir {}: {e}", debug_dir.display()))?;
        save_gray(
            &debug_dir.join("01_tone_base.png"),
            bundle.width,
            bundle.height,
            &buffers.tone_base,
        )?;
        save_gray(
            &debug_dir.join("02_stylized.png"),
            bundle.width,
            bundle.height,
            &buffers.stylized,
        )?;
        save_gray(
            &debug_dir.join("03_quantized.png"),
            bundle.width,
            bundle.height,
            &buffers.quantized,
        )?;
        if let Some(sun_map) = optional.sun_light.as_ref() {
            save_gray(
                &debug_dir.join("00_sun_relight.png"),
                bundle.width,
                bundle.height,
                sun_map,
            )?;
        }
    }

    Ok(())
}
