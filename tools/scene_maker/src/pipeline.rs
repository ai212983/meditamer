use image::imageops::FilterType;
use serde::Serialize;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use crate::{
    cli::{
        parse_build_args, next_value, print_help, BuildConfig, ChannelId, ChannelTemplate,
        ExplicitChannelPaths, CHANNELS,
    },
    format::{
        compression_name, decode_len_hint, encode_strip, payload_start, raw_len_from_strip,
        read_header, write_header, ChannelDescriptor, StripEntry,
    },
};

#[derive(Clone)]
struct ChannelData {
    template: ChannelTemplate,
    source: String,
    pixels: Vec<u8>,
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
    if cfg.width == 0 || cfg.height == 0 {
        return Err("width and height must be greater than zero".to_owned());
    }
    if cfg.strip_height == 0 {
        return Err("strip-height must be greater than zero".to_owned());
    }

    let width = cfg.width as usize;
    let height = cfg.height as usize;
    let total_px = width * height;
    let strip_count = div_ceil_u16(cfg.height, cfg.strip_height);

    if let Some(parent) = cfg.out_bundle.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create bundle dir: {e}"))?;
    }
    if let Some(parent) = cfg.metadata_out.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create metadata dir: {e}"))?;
    }

    let mut channels = Vec::with_capacity(CHANNELS.len());
    let explicit = ExplicitChannelPaths {
        albedo: cfg.albedo.clone(),
        light: cfg.light.clone(),
        ao: cfg.ao.clone(),
        depth: cfg.depth.clone(),
        edge: cfg.edge.clone(),
        mask: cfg.mask.clone(),
        stroke: cfg.stroke.clone(),
        normal_x: cfg.normal_x.clone(),
        normal_y: cfg.normal_y.clone(),
    };

    for template in CHANNELS {
        let requested = explicit.lookup(template.name);
        let resolved = resolve_channel_path(&cfg.input_dir, template.name, requested.as_deref());

        let ch = match resolved {
            Some(path) => {
                let img = load_grayscale_resized(&path, cfg.width, cfg.height)?;
                ChannelData {
                    template,
                    source: path.display().to_string(),
                    pixels: img,
                }
            }
            None if template.required => {
                return Err(format!(
                    "missing required map '{}'; expected {}.png",
                    template.name, template.name
                ));
            }
            None => ChannelData {
                template,
                source: "generated-default".to_owned(),
                pixels: vec![template.default_value; total_px],
            },
        };

        channels.push(ch);
    }

    if cfg.derive_edge {
        let edge_idx = channel_index(ChannelId::Edge);
        let needs_edge = channels[edge_idx].source == "generated-default";
        if needs_edge {
            let depth_idx = channel_index(ChannelId::Depth);
            let depth_non_default = channels[depth_idx].source != "generated-default";
            let source_pixels = if depth_non_default {
                &channels[depth_idx].pixels
            } else {
                &channels[channel_index(ChannelId::Albedo)].pixels
            };
            channels[edge_idx].pixels = sobel_edges(source_pixels, width, height);
            channels[edge_idx].source = if depth_non_default {
                "derived-from-depth".to_owned()
            } else {
                "derived-from-albedo".to_owned()
            };
        }
    }

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

    let mut per_channel_encoded: Vec<Vec<Vec<u8>>> = Vec::with_capacity(channels.len());
    let mut entries: Vec<StripEntry> = Vec::with_capacity(channels.len() * (strip_count as usize));

    for ch in &channels {
        let mut channel_strips = Vec::with_capacity(strip_count as usize);
        for strip_idx in 0..strip_count as usize {
            let y0 = strip_idx * cfg.strip_height as usize;
            let y1 = ((strip_idx + 1) * cfg.strip_height as usize).min(height);
            let rows = y1 - y0;
            let start = y0 * width;
            let end = start + rows * width;
            let raw = &ch.pixels[start..end];
            let encoded = encode_strip(raw, compression);
            channel_strips.push(encoded);
        }
        per_channel_encoded.push(channel_strips);
    }

    let mut payload_offset = payload_start(channel_descriptors.len(), strip_count as usize) as u64;

    for channel_strips in &per_channel_encoded {
        for strip in channel_strips {
            let length = strip.len() as u32;
            let raw_length =
                decode_len_hint(strip, compression).unwrap_or_else(|| raw_len_from_strip(strip, compression))
                    as u32;
            entries.push(StripEntry {
                offset: payload_offset,
                length,
                raw_length,
            });
            payload_offset += length as u64;
        }
    }

    let mut out = fs::File::create(&cfg.out_bundle)
        .map_err(|e| format!("create bundle {}: {e}", cfg.out_bundle.display()))?;

    write_header(
        &mut out,
        cfg.width,
        cfg.height,
        cfg.strip_height,
        strip_count,
        channel_descriptors.len() as u16,
    )?;

    for desc in &channel_descriptors {
        out.write_all(&[desc.id, desc.bits_per_pixel, desc.compression, desc.reserved])
            .map_err(|e| format!("write channel descriptor: {e}"))?;
    }

    for entry in &entries {
        out.write_all(&entry.offset.to_le_bytes())
            .map_err(|e| format!("write strip offset: {e}"))?;
        out.write_all(&entry.length.to_le_bytes())
            .map_err(|e| format!("write strip length: {e}"))?;
        out.write_all(&entry.raw_length.to_le_bytes())
            .map_err(|e| format!("write strip raw length: {e}"))?;
    }

    for channel_strips in &per_channel_encoded {
        for strip in channel_strips {
            out.write_all(strip)
                .map_err(|e| format!("write strip payload: {e}"))?;
        }
    }
    out.flush().map_err(|e| format!("flush bundle: {e}"))?;

    let bundle_bytes = fs::metadata(&cfg.out_bundle)
        .map_err(|e| format!("read bundle metadata: {e}"))?
        .len();

    let meta = Metadata {
        width: cfg.width,
        height: cfg.height,
        strip_height: cfg.strip_height,
        strip_count,
        compression: compression.as_str().to_owned(),
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
    };

    let meta_json =
        serde_json::to_string_pretty(&meta).map_err(|e| format!("serialize metadata: {e}"))?;
    fs::write(&cfg.metadata_out, meta_json)
        .map_err(|e| format!("write metadata {}: {e}", cfg.metadata_out.display()))?;

    println!("wrote bundle: {}", cfg.out_bundle.display());
    println!("wrote metadata: {}", cfg.metadata_out.display());
    println!(
        "scene: {}x{}, strips={}, compression={}",
        cfg.width,
        cfg.height,
        strip_count,
        compression.as_str()
    );

    Ok(())
}

pub(crate) fn run_inspect<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut bundle: Option<PathBuf> = None;
    let mut it = args.into_iter();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bundle" => bundle = Some(PathBuf::from(next_value("--bundle", &mut it)?)),
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            _ => return Err(format!("unknown arg for inspect: {arg}")),
        }
    }

    let bundle = bundle.unwrap_or_else(|| PathBuf::from("tools/scene_maker/out/scene.scenebundle"));
    let mut file =
        fs::File::open(&bundle).map_err(|e| format!("open bundle {}: {e}", bundle.display()))?;

    let (width, height, strip_height, strip_count, channel_count) = read_header(&mut file)?;
    println!("bundle: {}", bundle.display());
    println!("size: {}x{}", width, height);
    println!("strip height: {strip_height}, strip count: {strip_count}");
    println!("channels: {channel_count}");

    let mut descs = Vec::with_capacity(channel_count as usize);
    for idx in 0..channel_count {
        let mut b = [0u8; 4];
        file.read_exact(&mut b)
            .map_err(|e| format!("read channel descriptor {idx}: {e}"))?;
        descs.push(ChannelDescriptor {
            id: b[0],
            bits_per_pixel: b[1],
            compression: b[2],
            reserved: b[3],
        });
    }

    let entry_count = (channel_count as usize) * (strip_count as usize);
    let mut entries = Vec::with_capacity(entry_count);
    for idx in 0..entry_count {
        let mut off = [0u8; 8];
        let mut len = [0u8; 4];
        let mut raw = [0u8; 4];
        file.read_exact(&mut off)
            .map_err(|e| format!("read strip entry offset {idx}: {e}"))?;
        file.read_exact(&mut len)
            .map_err(|e| format!("read strip entry len {idx}: {e}"))?;
        file.read_exact(&mut raw)
            .map_err(|e| format!("read strip entry raw len {idx}: {e}"))?;
        entries.push(StripEntry {
            offset: u64::from_le_bytes(off),
            length: u32::from_le_bytes(len),
            raw_length: u32::from_le_bytes(raw),
        });
    }

    for (ch_idx, desc) in descs.iter().enumerate() {
        let mut encoded_total = 0u64;
        let mut raw_total = 0u64;
        for strip_idx in 0..strip_count as usize {
            let entry = entries[ch_idx * (strip_count as usize) + strip_idx];
            encoded_total += entry.length as u64;
            raw_total += entry.raw_length as u64;
        }
        let ratio = if raw_total == 0 {
            1.0
        } else {
            encoded_total as f32 / raw_total as f32
        };
        println!(
            "  channel id={} bpp={} compression={} encoded={} raw={} ratio={:.3}",
            desc.id,
            desc.bits_per_pixel,
            compression_name(desc.compression),
            encoded_total,
            raw_total,
            ratio
        );
    }

    Ok(())
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
