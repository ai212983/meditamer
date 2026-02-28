use image::imageops::FilterType;
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::{
    cli::{
        parse_build_args, BuildConfig, ChannelId, ChannelTemplate, ExplicitChannelPaths, CHANNELS,
    },
    format::{
        decode_len_hint, encode_strip, payload_start, raw_len_from_strip, write_header,
        BundleHeader, ChannelDescriptor, StripEntry,
    },
};

#[derive(Clone)]
struct ChannelData {
    template: ChannelTemplate,
    source: String,
    pixels: Vec<u8>,
}

#[derive(Clone, Copy)]
struct SceneDims {
    width: usize,
    height: usize,
    total_px: usize,
    strip_count: u16,
}

struct EncodedPayload {
    channel_descriptors: Vec<ChannelDescriptor>,
    entries: Vec<StripEntry>,
    per_channel_encoded: Vec<Vec<Vec<u8>>>,
}

#[derive(Serialize)]
struct Metadata {
    width: u16,
    height: u16,
    strip_height: u16,
    strip_count: u16,
    compression: String,
    bundle_bytes: u64,
    channels: Vec<MetadataChannel>,
}

#[derive(Serialize)]
struct MetadataChannel {
    id: u8,
    name: String,
    source: String,
    min: u8,
    max: u8,
    mean: f32,
}

pub(crate) fn run_build<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let cfg = parse_build_args(args)?;
    run_build_with_config(cfg)
}

fn run_build_with_config(cfg: BuildConfig) -> Result<(), String> {
    let dims = validate_build_config(&cfg)?;
    prepare_output_dirs(&cfg)?;
    let mut channels = load_channels(&cfg, dims.total_px)?;
    derive_edge_if_needed(&cfg, dims, &mut channels);
    let encoded = encode_channels(&cfg, dims, &channels);
    let bundle_bytes = write_bundle(&cfg, dims.strip_count, &encoded)?;
    let meta = build_metadata(&cfg, dims.strip_count, bundle_bytes, &channels);
    write_metadata(&cfg, &meta)?;

    println!("wrote bundle: {}", cfg.out_bundle.display());
    println!("wrote metadata: {}", cfg.metadata_out.display());
    println!(
        "scene: {}x{}, strips={}, compression={}",
        cfg.width,
        cfg.height,
        dims.strip_count,
        cfg.compression.as_str()
    );
    Ok(())
}

fn validate_build_config(cfg: &BuildConfig) -> Result<SceneDims, String> {
    if cfg.width == 0 || cfg.height == 0 {
        return Err("width and height must be greater than zero".to_owned());
    }
    if cfg.strip_height == 0 {
        return Err("strip-height must be greater than zero".to_owned());
    }

    let width = cfg.width as usize;
    let height = cfg.height as usize;
    Ok(SceneDims {
        width,
        height,
        total_px: width * height,
        strip_count: div_ceil_u16(cfg.height, cfg.strip_height),
    })
}

fn prepare_output_dirs(cfg: &BuildConfig) -> Result<(), String> {
    if let Some(parent) = cfg.out_bundle.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create bundle dir: {e}"))?;
    }
    if let Some(parent) = cfg.metadata_out.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create metadata dir: {e}"))?;
    }
    Ok(())
}

fn explicit_channel_paths(cfg: &BuildConfig) -> ExplicitChannelPaths {
    ExplicitChannelPaths {
        albedo: cfg.albedo.clone(),
        light: cfg.light.clone(),
        ao: cfg.ao.clone(),
        depth: cfg.depth.clone(),
        edge: cfg.edge.clone(),
        mask: cfg.mask.clone(),
        stroke: cfg.stroke.clone(),
        normal_x: cfg.normal_x.clone(),
        normal_y: cfg.normal_y.clone(),
    }
}

fn load_channels(cfg: &BuildConfig, total_px: usize) -> Result<Vec<ChannelData>, String> {
    let explicit = explicit_channel_paths(cfg);
    let mut channels = Vec::with_capacity(CHANNELS.len());
    for template in CHANNELS {
        channels.push(load_channel(cfg, &explicit, template, total_px)?);
    }
    Ok(channels)
}

fn load_channel(
    cfg: &BuildConfig,
    explicit: &ExplicitChannelPaths,
    template: ChannelTemplate,
    total_px: usize,
) -> Result<ChannelData, String> {
    let requested = explicit.lookup(template.name);
    let resolved = resolve_channel_path(&cfg.input_dir, template.name, requested.as_deref());

    match resolved {
        Some(path) => {
            let img = load_grayscale_resized(&path, cfg.width, cfg.height)?;
            Ok(ChannelData {
                template,
                source: path.display().to_string(),
                pixels: img,
            })
        }
        None if template.required => Err(format!(
            "missing required map '{}'; expected {}.png",
            template.name, template.name
        )),
        None => Ok(ChannelData {
            template,
            source: "generated-default".to_owned(),
            pixels: vec![template.default_value; total_px],
        }),
    }
}

fn derive_edge_if_needed(cfg: &BuildConfig, dims: SceneDims, channels: &mut [ChannelData]) {
    if !cfg.derive_edge {
        return;
    }

    let edge_idx = channel_index(ChannelId::Edge);
    if channels[edge_idx].source != "generated-default" {
        return;
    }

    let depth_idx = channel_index(ChannelId::Depth);
    let depth_non_default = channels[depth_idx].source != "generated-default";
    let source_pixels = if depth_non_default {
        &channels[depth_idx].pixels
    } else {
        &channels[channel_index(ChannelId::Albedo)].pixels
    };
    channels[edge_idx].pixels = sobel_edges(source_pixels, dims.width, dims.height);
    channels[edge_idx].source = if depth_non_default {
        "derived-from-depth".to_owned()
    } else {
        "derived-from-albedo".to_owned()
    };
}

fn encode_channels(cfg: &BuildConfig, dims: SceneDims, channels: &[ChannelData]) -> EncodedPayload {
    let compression = cfg.compression;
    let channel_descriptors: Vec<ChannelDescriptor> = channels
        .iter()
        .map(|ch| ChannelDescriptor {
            id: ch.template.id as u8,
            bits_per_pixel: 8,
            compression: compression.as_u8(),
            reserved: 0,
        })
        .collect();

    let per_channel_encoded = encode_channel_strips(cfg, dims, channels, compression);
    let entries = build_strip_entries(
        &per_channel_encoded,
        compression,
        channel_descriptors.len(),
        dims.strip_count as usize,
    );

    EncodedPayload {
        channel_descriptors,
        entries,
        per_channel_encoded,
    }
}

fn encode_channel_strips(
    cfg: &BuildConfig,
    dims: SceneDims,
    channels: &[ChannelData],
    compression: crate::cli::Compression,
) -> Vec<Vec<Vec<u8>>> {
    let mut per_channel_encoded: Vec<Vec<Vec<u8>>> = Vec::with_capacity(channels.len());
    for ch in channels {
        let mut channel_strips = Vec::with_capacity(dims.strip_count as usize);
        for strip_idx in 0..dims.strip_count as usize {
            let y0 = strip_idx * cfg.strip_height as usize;
            let y1 = ((strip_idx + 1) * cfg.strip_height as usize).min(dims.height);
            let rows = y1 - y0;
            let start = y0 * dims.width;
            let end = start + rows * dims.width;
            let raw = &ch.pixels[start..end];
            channel_strips.push(encode_strip(raw, compression));
        }
        per_channel_encoded.push(channel_strips);
    }
    per_channel_encoded
}

fn build_strip_entries(
    per_channel_encoded: &[Vec<Vec<u8>>],
    compression: crate::cli::Compression,
    channel_count: usize,
    strip_count: usize,
) -> Vec<StripEntry> {
    let mut entries: Vec<StripEntry> = Vec::with_capacity(channel_count * strip_count);
    let mut payload_offset = payload_start(channel_count, strip_count) as u64;
    for channel_strips in per_channel_encoded {
        for strip in channel_strips {
            let length = strip.len() as u32;
            let raw_length = decode_len_hint(strip, compression)
                .unwrap_or_else(|| raw_len_from_strip(strip, compression))
                as u32;
            entries.push(StripEntry {
                offset: payload_offset,
                length,
                raw_length,
            });
            payload_offset += length as u64;
        }
    }
    entries
}

fn write_bundle(
    cfg: &BuildConfig,
    strip_count: u16,
    encoded: &EncodedPayload,
) -> Result<u64, String> {
    let mut out = fs::File::create(&cfg.out_bundle)
        .map_err(|e| format!("create bundle {}: {e}", cfg.out_bundle.display()))?;
    write_bundle_header(
        &mut out,
        cfg,
        strip_count,
        encoded.channel_descriptors.len() as u16,
    )?;
    write_channel_descriptors(&mut out, &encoded.channel_descriptors)?;
    write_strip_entries(&mut out, &encoded.entries)?;
    write_strip_payload(&mut out, &encoded.per_channel_encoded)?;
    out.flush().map_err(|e| format!("flush bundle: {e}"))?;
    bundle_size(&cfg.out_bundle)
}

fn write_bundle_header(
    out: &mut fs::File,
    cfg: &BuildConfig,
    strip_count: u16,
    channel_count: u16,
) -> Result<(), String> {
    write_header(
        out,
        BundleHeader {
            width: cfg.width,
            height: cfg.height,
            strip_height: cfg.strip_height,
            strip_count,
            channel_count,
        },
    )
}

fn write_channel_descriptors(
    out: &mut fs::File,
    channel_descriptors: &[ChannelDescriptor],
) -> Result<(), String> {
    for desc in channel_descriptors {
        out.write_all(&[
            desc.id,
            desc.bits_per_pixel,
            desc.compression,
            desc.reserved,
        ])
        .map_err(|e| format!("write channel descriptor: {e}"))?;
    }
    Ok(())
}

fn write_strip_entries(out: &mut fs::File, entries: &[StripEntry]) -> Result<(), String> {
    for entry in entries {
        out.write_all(&entry.offset.to_le_bytes())
            .map_err(|e| format!("write strip offset: {e}"))?;
        out.write_all(&entry.length.to_le_bytes())
            .map_err(|e| format!("write strip length: {e}"))?;
        out.write_all(&entry.raw_length.to_le_bytes())
            .map_err(|e| format!("write strip raw length: {e}"))?;
    }
    Ok(())
}

fn write_strip_payload(
    out: &mut fs::File,
    per_channel_encoded: &[Vec<Vec<u8>>],
) -> Result<(), String> {
    for channel_strips in per_channel_encoded {
        for strip in channel_strips {
            out.write_all(strip)
                .map_err(|e| format!("write strip payload: {e}"))?;
        }
    }
    Ok(())
}

fn bundle_size(path: &Path) -> Result<u64, String> {
    fs::metadata(path)
        .map_err(|e| format!("read bundle metadata: {e}"))
        .map(|m| m.len())
}

fn build_metadata(
    cfg: &BuildConfig,
    strip_count: u16,
    bundle_bytes: u64,
    channels: &[ChannelData],
) -> Metadata {
    Metadata {
        width: cfg.width,
        height: cfg.height,
        strip_height: cfg.strip_height,
        strip_count,
        compression: cfg.compression.as_str().to_owned(),
        bundle_bytes,
        channels: channels
            .iter()
            .map(|ch| {
                let (min, max, mean) = stats(&ch.pixels);
                MetadataChannel {
                    id: ch.template.id as u8,
                    name: ch.template.name.to_owned(),
                    source: ch.source.clone(),
                    min,
                    max,
                    mean,
                }
            })
            .collect(),
    }
}

fn write_metadata(cfg: &BuildConfig, meta: &Metadata) -> Result<(), String> {
    let meta_json =
        serde_json::to_string_pretty(meta).map_err(|e| format!("serialize metadata: {e}"))?;
    fs::write(&cfg.metadata_out, meta_json)
        .map_err(|e| format!("write metadata {}: {e}", cfg.metadata_out.display()))
}

pub(crate) fn run_inspect<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    crate::inspect::run_inspect(args)
}

fn resolve_channel_path(input_dir: &Path, name: &str, explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        if path.exists() {
            return Some(path.to_path_buf());
        }
        return None;
    }

    let exts = ["png"];
    for ext in exts {
        let candidate = input_dir.join(format!("{name}.{ext}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn load_grayscale_resized(path: &Path, width: u16, height: u16) -> Result<Vec<u8>, String> {
    let img = image::open(path)
        .map_err(|e| format!("open image {}: {e}", path.display()))?
        .to_luma8();

    let out = if img.width() == width as u32 && img.height() == height as u32 {
        img
    } else {
        image::imageops::resize(&img, width as u32, height as u32, FilterType::CatmullRom)
    };

    Ok(out.into_raw())
}

fn sobel_edges(src: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];

    for y in 0..height {
        for x in 0..width {
            let p = |ix: i32, iy: i32| -> i32 {
                let xx = ix.clamp(0, (width as i32) - 1) as usize;
                let yy = iy.clamp(0, (height as i32) - 1) as usize;
                src[yy * width + xx] as i32
            };

            let x_i = x as i32;
            let y_i = y as i32;

            let gx = -p(x_i - 1, y_i - 1) + p(x_i + 1, y_i - 1) - 2 * p(x_i - 1, y_i)
                + 2 * p(x_i + 1, y_i)
                - p(x_i - 1, y_i + 1)
                + p(x_i + 1, y_i + 1);

            let gy = -p(x_i - 1, y_i - 1) - 2 * p(x_i, y_i - 1) - p(x_i + 1, y_i - 1)
                + p(x_i - 1, y_i + 1)
                + 2 * p(x_i, y_i + 1)
                + p(x_i + 1, y_i + 1);

            let mag = ((gx.abs() + gy.abs()) / 6).clamp(0, 255) as u8;
            out[y * width + x] = mag;
        }
    }

    out
}

fn stats(data: &[u8]) -> (u8, u8, f32) {
    if data.is_empty() {
        return (0, 0, 0.0);
    }
    let mut min = u8::MAX;
    let mut max = u8::MIN;
    let mut sum = 0u64;

    for &v in data {
        min = min.min(v);
        max = max.max(v);
        sum += v as u64;
    }

    (min, max, (sum as f32) / (data.len() as f32))
}

fn channel_index(id: ChannelId) -> usize {
    CHANNELS
        .iter()
        .position(|ch| ch.id as u8 == id as u8)
        .expect("channel id present")
}

fn div_ceil_u16(a: u16, b: u16) -> u16 {
    ((a as u32 + b as u32 - 1) / b as u32) as u16
}

#[cfg(test)]
mod tests;
