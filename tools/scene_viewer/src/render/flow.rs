//! Render orchestration: decode the bundle's channels, run the passes, write the
//! outputs. The per-render data the stages hand to each other lives here.

mod inputs;
mod output;
mod pass;

use super::{
    get_channel_or_default, CH_ALBEDO, CH_AO, CH_DEPTH, CH_EDGE, CH_LIGHT, CH_MASK, CH_NORMAL_X,
    CH_NORMAL_Y, CH_STROKE,
};
use crate::{
    bundle::{load_bundle, Bundle},
    cli::{mode_name, Config},
};
use inputs::{dump_channels_if_requested, prepare_optional_maps};
use output::save_render_outputs;
use pass::render_frame;

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

fn collect_input_channels(bundle: &Bundle, total: usize) -> Result<RenderInputs<'_>, String> {
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

fn collect_normal_xy(bundle: &Bundle, total: usize) -> Option<(&[u8], &[u8])> {
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
