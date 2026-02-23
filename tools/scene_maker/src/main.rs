use image::imageops::FilterType;
use serde::Serialize;
use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 8] = b"SMBNDL1\0";
const VERSION: u16 = 1;
const HEADER_LEN: u16 = 24;
const CHANNEL_DESC_LEN: usize = 4;
const STRIP_ENTRY_LEN: usize = 16;

#[derive(Clone, Copy, Debug)]
enum Compression {
    None,
    Rle,
}

impl Compression {
    fn as_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Rle => 1,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Rle => "rle",
        }
    }

    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "none" => Ok(Self::None),
            "rle" => Ok(Self::Rle),
            _ => Err(format!("invalid compression '{raw}', expected none|rle")),
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
enum ChannelId {
    Albedo = 1,
    Light = 2,
    Ao = 3,
    Depth = 4,
    Edge = 5,
    Mask = 6,
    Stroke = 7,
    NormalX = 8,
    NormalY = 9,
}

#[derive(Clone, Copy)]
struct ChannelTemplate {
    id: ChannelId,
    name: &'static str,
    required: bool,
    default_value: u8,
}

const CHANNELS: [ChannelTemplate; 9] = [
    ChannelTemplate {
        id: ChannelId::Albedo,
        name: "albedo",
        required: true,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Light,
        name: "light",
        required: true,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Ao,
        name: "ao",
        required: false,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Depth,
        name: "depth",
        required: false,
        default_value: 0,
    },
    ChannelTemplate {
        id: ChannelId::Edge,
        name: "edge",
        required: false,
        default_value: 0,
    },
    ChannelTemplate {
        id: ChannelId::Mask,
        name: "mask",
        required: false,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Stroke,
        name: "stroke",
        required: false,
        default_value: 128,
    },
    ChannelTemplate {
        id: ChannelId::NormalX,
        name: "normal_x",
        required: false,
        default_value: 128,
    },
    ChannelTemplate {
        id: ChannelId::NormalY,
        name: "normal_y",
        required: false,
        default_value: 128,
    },
];

#[derive(Clone)]
struct BuildConfig {
    input_dir: PathBuf,
    out_bundle: PathBuf,
    metadata_out: PathBuf,
    width: u16,
    height: u16,
    strip_height: u16,
    compression: Compression,
    derive_edge: bool,
    albedo: Option<PathBuf>,
    light: Option<PathBuf>,
    ao: Option<PathBuf>,
    depth: Option<PathBuf>,
    edge: Option<PathBuf>,
    mask: Option<PathBuf>,
    stroke: Option<PathBuf>,
    normal_x: Option<PathBuf>,
    normal_y: Option<PathBuf>,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            input_dir: PathBuf::from("tools/scene_maker/input"),
            out_bundle: PathBuf::from("tools/scene_maker/out/scene.scenebundle"),
            metadata_out: PathBuf::from("tools/scene_maker/out/scene.scenebundle.json"),
            width: 600,
            height: 600,
            strip_height: 32,
            compression: Compression::Rle,
            derive_edge: true,
            albedo: None,
            light: None,
            ao: None,
            depth: None,
            edge: None,
            mask: None,
            stroke: None,
            normal_x: None,
            normal_y: None,
        }
    }
}

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

#[derive(Clone, Copy)]
struct ChannelDescriptor {
    id: u8,
    bits_per_pixel: u8,
    compression: u8,
    reserved: u8,
}

#[derive(Clone, Copy)]
struct StripEntry {
    offset: u64,
    length: u32,
    raw_length: u32,
}

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        print_help();
        return Ok(());
    };

    match cmd.as_str() {
        "build" => run_build(args),
        "inspect" => run_inspect(args),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        _ => Err(format!("unknown command '{cmd}'")),
    }
}

fn run_build<I>(args: I) -> Result<(), String>
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

    let header_bytes = HEADER_LEN as usize;
    let channel_desc_bytes = channel_descriptors.len() * CHANNEL_DESC_LEN;
    let strip_entry_count = (strip_count as usize) * channels.len();
    let strip_entry_bytes = strip_entry_count * STRIP_ENTRY_LEN;
    let mut payload_offset = (header_bytes + channel_desc_bytes + strip_entry_bytes) as u64;

    for channel_strips in &per_channel_encoded {
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
        out.write_all(&[
            desc.id,
            desc.bits_per_pixel,
            desc.compression,
            desc.reserved,
        ])
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

fn run_inspect<I>(args: I) -> Result<(), String>
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

fn parse_build_args<I>(args: I) -> Result<BuildConfig, String>
where
    I: IntoIterator<Item = String>,
{
    let mut cfg = BuildConfig::default();
    let mut it = args.into_iter();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--input" => cfg.input_dir = PathBuf::from(next_value("--input", &mut it)?),
            "--out" => {
                cfg.out_bundle = PathBuf::from(next_value("--out", &mut it)?);
                cfg.metadata_out = cfg.out_bundle.with_extension("scenebundle.json");
            }
            "--metadata" => cfg.metadata_out = PathBuf::from(next_value("--metadata", &mut it)?),
            "--width" => cfg.width = parse_num(next_value("--width", &mut it)?, "--width")?,
            "--height" => cfg.height = parse_num(next_value("--height", &mut it)?, "--height")?,
            "--strip-height" => {
                cfg.strip_height =
                    parse_num(next_value("--strip-height", &mut it)?, "--strip-height")?
            }
            "--compression" => {
                cfg.compression = Compression::from_str(&next_value("--compression", &mut it)?)?
            }
            "--derive-edge" => {
                cfg.derive_edge = parse_bool(&next_value("--derive-edge", &mut it)?)?;
            }
            "--albedo" => cfg.albedo = Some(PathBuf::from(next_value("--albedo", &mut it)?)),
            "--light" => cfg.light = Some(PathBuf::from(next_value("--light", &mut it)?)),
            "--ao" => cfg.ao = Some(PathBuf::from(next_value("--ao", &mut it)?)),
            "--depth" => cfg.depth = Some(PathBuf::from(next_value("--depth", &mut it)?)),
            "--edge" => cfg.edge = Some(PathBuf::from(next_value("--edge", &mut it)?)),
            "--mask" => cfg.mask = Some(PathBuf::from(next_value("--mask", &mut it)?)),
            "--stroke" => cfg.stroke = Some(PathBuf::from(next_value("--stroke", &mut it)?)),
            "--normal-x" => cfg.normal_x = Some(PathBuf::from(next_value("--normal-x", &mut it)?)),
            "--normal-y" => cfg.normal_y = Some(PathBuf::from(next_value("--normal-y", &mut it)?)),
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("unknown arg for build: {arg}")),
        }
    }

    Ok(cfg)
}

struct ExplicitChannelPaths {
    albedo: Option<PathBuf>,
    light: Option<PathBuf>,
    ao: Option<PathBuf>,
    depth: Option<PathBuf>,
    edge: Option<PathBuf>,
    mask: Option<PathBuf>,
    stroke: Option<PathBuf>,
    normal_x: Option<PathBuf>,
    normal_y: Option<PathBuf>,
}

impl ExplicitChannelPaths {
    fn lookup(&self, name: &str) -> Option<PathBuf> {
        match name {
            "albedo" => self.albedo.clone(),
            "light" => self.light.clone(),
            "ao" => self.ao.clone(),
            "depth" => self.depth.clone(),
            "edge" => self.edge.clone(),
            "mask" => self.mask.clone(),
            "stroke" => self.stroke.clone(),
            "normal_x" => self.normal_x.clone(),
            "normal_y" => self.normal_y.clone(),
            _ => None,
        }
    }
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

fn encode_strip(raw: &[u8], compression: Compression) -> Vec<u8> {
    match compression {
        Compression::None => raw.to_vec(),
        Compression::Rle => rle_encode(raw),
    }
}

fn decode_len_hint(strip: &[u8], compression: Compression) -> Option<usize> {
    match compression {
        Compression::None => Some(strip.len()),
        Compression::Rle => {
            if strip.len() % 2 != 0 {
                return None;
            }
            let mut len = 0usize;
            let mut i = 0usize;
            while i < strip.len() {
                len += strip[i] as usize;
                i += 2;
            }
            Some(len)
        }
    }
}

fn raw_len_from_strip(strip: &[u8], compression: Compression) -> usize {
    match compression {
        Compression::None => strip.len(),
        Compression::Rle => strip
            .chunks_exact(2)
            .map(|pair| pair[0] as usize)
            .sum::<usize>(),
    }
}

fn rle_encode(raw: &[u8]) -> Vec<u8> {
    if raw.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(raw.len() / 2);
    let mut i = 0;
    while i < raw.len() {
        let value = raw[i];
        let mut run = 1usize;
        while i + run < raw.len() && raw[i + run] == value && run < 255 {
            run += 1;
        }
        out.push(run as u8);
        out.push(value);
        i += run;
    }
    out
}

fn write_header<W: Write>(
    mut out: W,
    width: u16,
    height: u16,
    strip_height: u16,
    strip_count: u16,
    channel_count: u16,
) -> Result<(), String> {
    out.write_all(MAGIC)
        .map_err(|e| format!("write header magic: {e}"))?;
    out.write_all(&VERSION.to_le_bytes())
        .map_err(|e| format!("write header version: {e}"))?;
    out.write_all(&HEADER_LEN.to_le_bytes())
        .map_err(|e| format!("write header len: {e}"))?;
    out.write_all(&width.to_le_bytes())
        .map_err(|e| format!("write header width: {e}"))?;
    out.write_all(&height.to_le_bytes())
        .map_err(|e| format!("write header height: {e}"))?;
    out.write_all(&strip_height.to_le_bytes())
        .map_err(|e| format!("write header strip height: {e}"))?;
    out.write_all(&strip_count.to_le_bytes())
        .map_err(|e| format!("write header strip count: {e}"))?;
    out.write_all(&channel_count.to_le_bytes())
        .map_err(|e| format!("write header channel count: {e}"))?;
    out.write_all(&0u16.to_le_bytes())
        .map_err(|e| format!("write header flags: {e}"))?;
    Ok(())
}

fn read_header<R: Read>(mut r: R) -> Result<(u16, u16, u16, u16, u16), String> {
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)
        .map_err(|e| format!("read header magic: {e}"))?;
    if &magic != MAGIC {
        return Err("invalid magic; not a scene bundle".to_owned());
    }

    let version = read_u16(&mut r, "version")?;
    if version != VERSION {
        return Err(format!("unsupported bundle version {version}"));
    }

    let header_len = read_u16(&mut r, "header_len")?;
    if header_len as usize != HEADER_LEN as usize {
        return Err(format!("unsupported header length {header_len}"));
    }

    let width = read_u16(&mut r, "width")?;
    let height = read_u16(&mut r, "height")?;
    let strip_height = read_u16(&mut r, "strip_height")?;
    let strip_count = read_u16(&mut r, "strip_count")?;
    let channel_count = read_u16(&mut r, "channel_count")?;
    let _flags = read_u16(&mut r, "flags")?;

    Ok((width, height, strip_height, strip_count, channel_count))
}

fn read_u16<R: Read>(r: &mut R, what: &str) -> Result<u16, String> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)
        .map_err(|e| format!("read {what}: {e}"))?;
    Ok(u16::from_le_bytes(buf))
}

fn compression_name(code: u8) -> &'static str {
    match code {
        0 => "none",
        1 => "rle",
        _ => "unknown",
    }
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

fn next_value<I>(flag: &str, it: &mut I) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    it.next().ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_num<T>(raw: String, name: &str) -> Result<T, String>
where
    T: core::str::FromStr,
{
    raw.parse::<T>()
        .map_err(|_| format!("invalid numeric value for {name}: {raw}"))
}

fn parse_bool(raw: &str) -> Result<bool, String> {
    match raw {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!("invalid bool '{raw}', expected true|false")),
    }
}

fn print_help() {
    println!(
        "scene_maker\n\n
actions:\n  build   Pack pre-baked map images into a strip-major .scenebundle\n  inspect Inspect bundle metadata/compression summary\n\n
action: build\n  --input DIR            Input directory (default: tools/scene_maker/input)\n  --out FILE             Output bundle path (default: tools/scene_maker/out/scene.scenebundle)\n  --metadata FILE        Output metadata json path\n  --width N              Target width (default: 600)\n  --height N             Target height (default: 600)\n  --strip-height N       Strip height in rows (default: 32)\n  --compression MODE     none|rle (default: rle)\n  --derive-edge BOOL     true|false (default: true)\n  --albedo FILE          Override albedo map path\n  --light FILE           Override light map path\n  --ao FILE              Override ao map path\n  --depth FILE           Override depth map path\n  --edge FILE            Override edge map path\n  --mask FILE            Override mask map path\n  --stroke FILE          Override stroke map path\n  --normal-x FILE        Override normal_x map path\n  --normal-y FILE        Override normal_y map path\n\n  If overrides are not set, files are discovered in --input using names:\n  albedo/light/ao/depth/edge/mask/stroke/normal_x/normal_y + extension .png\n\n
action: inspect\n  --bundle FILE          Bundle to inspect (default: tools/scene_maker/out/scene.scenebundle)"
    );
}

#[allow(dead_code)]
fn io_err_string(e: io::Error) -> String {
    e.to_string()
}
