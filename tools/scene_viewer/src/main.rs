use image::{GrayImage, ImageBuffer};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 8] = b"SMBNDL1\0";
const VERSION: u16 = 1;
const HEADER_LEN: u16 = 24;

#[derive(Clone, Copy, Debug)]
enum DitherMode {
    None,
    Bayer4,
}

impl DitherMode {
    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "none" => Ok(Self::None),
            "bayer4" => Ok(Self::Bayer4),
            _ => Err(format!("invalid dither '{raw}', expected none|bayer4")),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum OutputMode {
    Mono1,
    Gray3,
    Gray4,
    Gray8,
}

impl OutputMode {
    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "mono1" => Ok(Self::Mono1),
            "gray3" => Ok(Self::Gray3),
            "gray4" => Ok(Self::Gray4),
            "gray8" => Ok(Self::Gray8),
            _ => Err(format!(
                "invalid mode '{raw}', expected mono1|gray3|gray4|gray8"
            )),
        }
    }

    fn levels(self) -> u16 {
        match self {
            Self::Mono1 => 2,
            Self::Gray3 => 8,
            Self::Gray4 => 16,
            Self::Gray8 => 256,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ToneCurve {
    Linear,
    Wash,
    Filmic,
    SumiE,
}

impl ToneCurve {
    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "linear" => Ok(Self::Linear),
            "wash" => Ok(Self::Wash),
            "filmic" => Ok(Self::Filmic),
            "sumi-e" => Ok(Self::SumiE),
            _ => Err(format!(
                "invalid tone curve '{raw}', expected linear|wash|filmic|sumi-e"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RenderPreset {
    SumiE,
}

impl RenderPreset {
    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "sumi-e" => Ok(Self::SumiE),
            _ => Err(format!("invalid preset '{raw}', expected sumi-e")),
        }
    }
}

#[derive(Clone)]
struct Config {
    bundle: PathBuf,
    out: PathBuf,
    mode: OutputMode,
    dither: DitherMode,
    edge_strength: u8,
    fog_strength: u8,
    stroke_strength: u8,
    paper_strength: u8,
    tone_curve: ToneCurve,
    sun_strength: u8,
    sun_azimuth_deg: f32,
    sun_elevation_deg: f32,
    save_debug: Option<PathBuf>,
    dump_channels: Option<PathBuf>,
    ghost_from: Option<PathBuf>,
    ghost_alpha: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bundle: PathBuf::from("tools/scene_maker/out/scene.scenebundle"),
            out: PathBuf::from("tools/scene_viewer/out/render.png"),
            mode: OutputMode::Gray3,
            dither: DitherMode::Bayer4,
            edge_strength: 96,
            fog_strength: 72,
            stroke_strength: 24,
            paper_strength: 18,
            tone_curve: ToneCurve::Wash,
            sun_strength: 0,
            sun_azimuth_deg: 315.0,
            sun_elevation_deg: 35.0,
            save_debug: None,
            dump_channels: None,
            ghost_from: None,
            ghost_alpha: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct ChannelDesc {
    id: u8,
    bits_per_pixel: u8,
    compression: u8,
    _reserved: u8,
}

#[derive(Clone, Copy)]
struct StripEntry {
    offset: u64,
    length: u32,
    raw_length: u32,
}

struct Bundle {
    width: u16,
    height: u16,
    strip_height: u16,
    strip_count: u16,
    channels: HashMap<u8, Vec<u8>>,
}

const CH_ALBEDO: u8 = 1;
const CH_LIGHT: u8 = 2;
const CH_AO: u8 = 3;
const CH_DEPTH: u8 = 4;
const CH_EDGE: u8 = 5;
const CH_MASK: u8 = 6;
const CH_STROKE: u8 = 7;
const CH_NORMAL_X: u8 = 8;
const CH_NORMAL_Y: u8 = 9;

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        print_help();
        return Ok(());
    };

    match cmd.as_str() {
        "render" => run_render(args),
        "inspect" => run_inspect(args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        _ => Err(format!("unknown command '{cmd}'")),
    }
}

fn run_render<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let cfg = parse_render_args(args)?;
    let bundle = load_bundle(&cfg.bundle)?;
    let width = bundle.width as usize;
    let height = bundle.height as usize;
    let total = width * height;

    let albedo = get_channel_or_default(&bundle.channels, CH_ALBEDO, total, 255)?;
    let light = get_channel_or_default(&bundle.channels, CH_LIGHT, total, 255)?;
    let ao = get_channel_or_default(&bundle.channels, CH_AO, total, 255)?;
    let depth = get_channel_or_default(&bundle.channels, CH_DEPTH, total, 0)?;
    let edge = get_channel_or_default(&bundle.channels, CH_EDGE, total, 0)?;
    let mask = get_channel_or_default(&bundle.channels, CH_MASK, total, 255)?;
    let stroke = get_channel_or_default(&bundle.channels, CH_STROKE, total, 128)?;
    let normal_x_raw = bundle.channels.get(&CH_NORMAL_X);
    let normal_y_raw = bundle.channels.get(&CH_NORMAL_Y);
    let normal_xy = match (normal_x_raw, normal_y_raw) {
        (Some(nx), Some(ny)) if nx.len() == total && ny.len() == total => {
            let has_detail = nx.iter().zip(ny.iter()).any(|(&x, &y)| x != 128 || y != 128);
            if has_detail {
                Some((nx.as_slice(), ny.as_slice()))
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(ref out_dir) = cfg.dump_channels {
        fs::create_dir_all(out_dir)
            .map_err(|e| format!("create dump channels dir {}: {e}", out_dir.display()))?;
        save_gray(
            &out_dir.join("albedo.png"),
            bundle.width,
            bundle.height,
            albedo,
        )?;
        save_gray(
            &out_dir.join("light.png"),
            bundle.width,
            bundle.height,
            light,
        )?;
        save_gray(&out_dir.join("ao.png"), bundle.width, bundle.height, ao)?;
        save_gray(
            &out_dir.join("depth.png"),
            bundle.width,
            bundle.height,
            depth,
        )?;
        save_gray(&out_dir.join("edge.png"), bundle.width, bundle.height, edge)?;
        save_gray(&out_dir.join("mask.png"), bundle.width, bundle.height, mask)?;
        save_gray(
            &out_dir.join("stroke.png"),
            bundle.width,
            bundle.height,
            stroke,
        )?;
        if let Some((nx, ny)) = normal_xy {
            save_gray(&out_dir.join("normal_x.png"), bundle.width, bundle.height, nx)?;
            save_gray(&out_dir.join("normal_y.png"), bundle.width, bundle.height, ny)?;
        }
    }

    let tone_lut = build_tone_lut(cfg.tone_curve);
    let mut tone_base = vec![0u8; total];
    let mut stylized = vec![0u8; total];
    let mut quantized = vec![0u8; total];
    let sun_light = if cfg.sun_strength > 0 {
        Some(build_depth_relit_map(
            depth,
            normal_xy,
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

    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let light_shaded = if let Some(sun_map) = sun_light.as_ref() {
                mix_u8(light[i], sun_map[i], cfg.sun_strength)
            } else {
                light[i]
            };
            let base = mul8(mul8(albedo[i], light_shaded), ao[i]);
            tone_base[i] = base;

            let fog = mul8(depth[i], cfg.fog_strength);
            let fogged = mix_u8(base, 255, fog);

            let dark = mul8(edge[i], cfg.edge_strength);
            let edged = fogged.saturating_sub(dark);

            let stroke_delta = ink_brush_delta(
                i,
                x,
                y,
                stroke[i],
                edge[i],
                depth[i],
                normal_xy,
                cfg.stroke_strength,
            );
            let stroked = clamp_i16_to_u8((edged as i16) + stroke_delta);

            let paper_delta = ((paper_noise_u8(x as i32, y as i32) as i16) - 128)
                * (cfg.paper_strength as i16)
                / 255;
            let papered = clamp_i16_to_u8((stroked as i16) + paper_delta);

            let curved = tone_lut[papered as usize];
            let masked = mix_u8(255, curved, mask[i]);

            let ghosted = if let Some(prev) = ghost_prev.as_ref() {
                mix_u8(masked, prev[i], cfg.ghost_alpha)
            } else {
                masked
            };

            stylized[i] = ghosted;
            quantized[i] = quantize_u8(ghosted, x as i32, y as i32, cfg.mode, cfg.dither);
        }
    }

    if let Some(parent) = cfg.out.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create output dir {}: {e}", parent.display()))?;
    }
    save_gray(&cfg.out, bundle.width, bundle.height, &quantized)?;
    println!("wrote {}", cfg.out.display());

    if let Some(ref debug_dir) = cfg.save_debug {
        fs::create_dir_all(debug_dir)
            .map_err(|e| format!("create debug dir {}: {e}", debug_dir.display()))?;
        save_gray(
            &debug_dir.join("01_tone_base.png"),
            bundle.width,
            bundle.height,
            &tone_base,
        )?;
        save_gray(
            &debug_dir.join("02_stylized.png"),
            bundle.width,
            bundle.height,
            &stylized,
        )?;
        save_gray(
            &debug_dir.join("03_quantized.png"),
            bundle.width,
            bundle.height,
            &quantized,
        )?;
        if let Some(sun_map) = sun_light.as_ref() {
            save_gray(
                &debug_dir.join("00_sun_relight.png"),
                bundle.width,
                bundle.height,
                sun_map,
            )?;
        }
    }

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

fn run_inspect<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut bundle = PathBuf::from("tools/scene_maker/out/scene.scenebundle");
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bundle" => bundle = PathBuf::from(next_value("--bundle", &mut it)?),
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            _ => return Err(format!("unknown inspect arg '{arg}'")),
        }
    }

    let b = load_bundle(&bundle)?;
    println!("bundle: {}", bundle.display());
    println!("size: {}x{}", b.width, b.height);
    println!("strip_height: {}", b.strip_height);
    println!("strip_count: {}", b.strip_count);
    println!("channels: {}", b.channels.len());

    for (id, data) in &b.channels {
        println!("  id={id} bytes={}", data.len());
    }

    Ok(())
}

fn parse_render_args<I>(args: I) -> Result<Config, String>
where
    I: IntoIterator<Item = String>,
{
    let mut cfg = Config::default();
    let mut it = args.into_iter();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--preset" => apply_preset(
                &mut cfg,
                RenderPreset::from_str(&next_value("--preset", &mut it)?)?,
            ),
            "--bundle" => cfg.bundle = PathBuf::from(next_value("--bundle", &mut it)?),
            "--out" => cfg.out = PathBuf::from(next_value("--out", &mut it)?),
            "--mode" => cfg.mode = OutputMode::from_str(&next_value("--mode", &mut it)?)?,
            "--dither" => cfg.dither = DitherMode::from_str(&next_value("--dither", &mut it)?)?,
            "--edge-strength" => {
                cfg.edge_strength =
                    parse_num(next_value("--edge-strength", &mut it)?, "--edge-strength")?
            }
            "--fog-strength" => {
                cfg.fog_strength =
                    parse_num(next_value("--fog-strength", &mut it)?, "--fog-strength")?
            }
            "--stroke-strength" => {
                cfg.stroke_strength = parse_num(
                    next_value("--stroke-strength", &mut it)?,
                    "--stroke-strength",
                )?
            }
            "--paper-strength" => {
                cfg.paper_strength =
                    parse_num(next_value("--paper-strength", &mut it)?, "--paper-strength")?
            }
            "--tone-curve" => {
                cfg.tone_curve = ToneCurve::from_str(&next_value("--tone-curve", &mut it)?)?
            }
            "--sun-strength" => {
                cfg.sun_strength =
                    parse_num(next_value("--sun-strength", &mut it)?, "--sun-strength")?
            }
            "--sun-azimuth-deg" => {
                cfg.sun_azimuth_deg = parse_num(
                    next_value("--sun-azimuth-deg", &mut it)?,
                    "--sun-azimuth-deg",
                )?
            }
            "--sun-elevation-deg" => {
                cfg.sun_elevation_deg = parse_num(
                    next_value("--sun-elevation-deg", &mut it)?,
                    "--sun-elevation-deg",
                )?
            }
            "--save-debug" => {
                cfg.save_debug = Some(PathBuf::from(next_value("--save-debug", &mut it)?))
            }
            "--dump-channels" => {
                cfg.dump_channels = Some(PathBuf::from(next_value("--dump-channels", &mut it)?))
            }
            "--ghost-from" => {
                cfg.ghost_from = Some(PathBuf::from(next_value("--ghost-from", &mut it)?))
            }
            "--ghost-alpha" => {
                cfg.ghost_alpha = parse_num(next_value("--ghost-alpha", &mut it)?, "--ghost-alpha")?
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("unknown render arg '{arg}'")),
        }
    }

    Ok(cfg)
}

fn load_bundle(path: &Path) -> Result<Bundle, String> {
    let bytes = fs::read(path).map_err(|e| format!("read bundle {}: {e}", path.display()))?;
    let mut offset = 0usize;

    let magic = read_bytes(&bytes, &mut offset, 8, "magic")?;
    if magic != MAGIC {
        return Err("invalid bundle magic".to_owned());
    }

    let version = read_u16(&bytes, &mut offset, "version")?;
    if version != VERSION {
        return Err(format!("unsupported bundle version {version}"));
    }

    let header_len = read_u16(&bytes, &mut offset, "header_len")?;
    if header_len != HEADER_LEN {
        return Err(format!("unsupported header length {header_len}"));
    }

    let width = read_u16(&bytes, &mut offset, "width")?;
    let height = read_u16(&bytes, &mut offset, "height")?;
    let strip_height = read_u16(&bytes, &mut offset, "strip_height")?;
    let strip_count = read_u16(&bytes, &mut offset, "strip_count")?;
    let channel_count = read_u16(&bytes, &mut offset, "channel_count")?;
    let _flags = read_u16(&bytes, &mut offset, "flags")?;

    let mut descs = Vec::with_capacity(channel_count as usize);
    for i in 0..channel_count as usize {
        let raw = read_bytes(&bytes, &mut offset, 4, &format!("channel desc {i}"))?;
        descs.push(ChannelDesc {
            id: raw[0],
            bits_per_pixel: raw[1],
            compression: raw[2],
            _reserved: raw[3],
        });
    }

    let entry_count = (channel_count as usize) * (strip_count as usize);
    let mut entries = Vec::with_capacity(entry_count);
    for i in 0..entry_count {
        let off = read_u64(&bytes, &mut offset, &format!("strip entry {i} offset"))?;
        let len = read_u32(&bytes, &mut offset, &format!("strip entry {i} len"))?;
        let raw_len = read_u32(&bytes, &mut offset, &format!("strip entry {i} raw_len"))?;
        entries.push(StripEntry {
            offset: off,
            length: len,
            raw_length: raw_len,
        });
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let mut channels = HashMap::with_capacity(channel_count as usize);

    for (ch_idx, desc) in descs.iter().enumerate() {
        if desc.bits_per_pixel != 8 {
            return Err(format!(
                "unsupported bits_per_pixel={} for channel id={}",
                desc.bits_per_pixel, desc.id
            ));
        }

        let mut decoded = Vec::with_capacity(width_usize * height_usize);
        for strip_idx in 0..strip_count as usize {
            let entry = entries[ch_idx * (strip_count as usize) + strip_idx];
            let start = entry.offset as usize;
            let end = start + entry.length as usize;
            if end > bytes.len() {
                return Err(format!(
                    "invalid strip bounds for channel id={} strip={strip_idx}",
                    desc.id
                ));
            }

            let payload = &bytes[start..end];
            let expected_rows = strip_rows(
                strip_idx,
                strip_count as usize,
                strip_height as usize,
                height_usize,
            );
            let expected_len = expected_rows * width_usize;
            if entry.raw_length as usize != expected_len {
                return Err(format!(
                    "strip raw_length mismatch channel id={} strip={} expected={} got={}",
                    desc.id, strip_idx, expected_len, entry.raw_length
                ));
            }

            let mut strip = decode_strip(payload, entry.raw_length as usize, desc.compression)
                .map_err(|e| format!("decode channel id={} strip={strip_idx}: {e}", desc.id))?;
            decoded.append(&mut strip);
        }

        if decoded.len() != width_usize * height_usize {
            return Err(format!(
                "decoded channel id={} size mismatch expected={} got={}",
                desc.id,
                width_usize * height_usize,
                decoded.len()
            ));
        }

        channels.insert(desc.id, decoded);
    }

    Ok(Bundle {
        width,
        height,
        strip_height,
        strip_count,
        channels,
    })
}

fn strip_rows(strip_idx: usize, strip_count: usize, strip_height: usize, height: usize) -> usize {
    let y0 = strip_idx * strip_height;
    let y1 = ((strip_idx + 1) * strip_height).min(height);
    if strip_idx >= strip_count || y0 >= height {
        0
    } else {
        y1 - y0
    }
}

fn decode_strip(payload: &[u8], expected_len: usize, compression: u8) -> Result<Vec<u8>, String> {
    match compression {
        0 => {
            if payload.len() != expected_len {
                return Err(format!(
                    "raw strip length mismatch expected={} got={}",
                    expected_len,
                    payload.len()
                ));
            }
            Ok(payload.to_vec())
        }
        1 => rle_decode(payload, expected_len),
        _ => Err(format!("unsupported compression code {compression}")),
    }
}

fn rle_decode(payload: &[u8], expected_len: usize) -> Result<Vec<u8>, String> {
    if payload.len() % 2 != 0 {
        return Err("rle payload must have even length".to_owned());
    }

    let mut out = Vec::with_capacity(expected_len);
    let mut i = 0usize;
    while i < payload.len() {
        let run = payload[i] as usize;
        let value = payload[i + 1];
        i += 2;

        if run == 0 {
            return Err("rle run length 0 is invalid".to_owned());
        }

        for _ in 0..run {
            out.push(value);
        }
    }

    if out.len() != expected_len {
        return Err(format!(
            "rle decoded length mismatch expected={} got={}",
            expected_len,
            out.len()
        ));
    }

    Ok(out)
}

fn get_channel_or_default<'a>(
    channels: &'a HashMap<u8, Vec<u8>>,
    id: u8,
    len: usize,
    default_value: u8,
) -> Result<&'a [u8], String> {
    if let Some(ch) = channels.get(&id) {
        if ch.len() != len {
            return Err(format!(
                "channel id={id} length mismatch expected={} got={}",
                len,
                ch.len()
            ));
        }
        Ok(ch)
    } else {
        // Return a leaked backing buffer to keep interface simple and no allocations in the hot loop.
        let boxed = vec![default_value; len].into_boxed_slice();
        Ok(Box::leak(boxed))
    }
}

fn quantize_u8(v: u8, x: i32, y: i32, mode: OutputMode, dither: DitherMode) -> u8 {
    match mode {
        OutputMode::Gray8 => v,
        OutputMode::Mono1 => {
            let threshold = match dither {
                DitherMode::None => 128,
                DitherMode::Bayer4 => bayer4_threshold_u8(x, y),
            };
            if v <= threshold {
                0
            } else {
                255
            }
        }
        OutputMode::Gray3 => {
            let adjusted = dither_adjust(v, x, y, dither, 4);
            quantize_levels(adjusted, 8)
        }
        OutputMode::Gray4 => {
            let adjusted = dither_adjust(v, x, y, dither, 2);
            quantize_levels(adjusted, 16)
        }
    }
}

fn quantize_levels(v: u8, levels: u16) -> u8 {
    if levels <= 1 {
        return v;
    }

    let max = levels - 1;
    let level = ((v as u32 * max as u32 + 127) / 255) as u16;
    ((level as u32 * 255 + (max as u32 / 2)) / max as u32) as u8
}

fn dither_adjust(v: u8, x: i32, y: i32, dither: DitherMode, strength: i16) -> u8 {
    let delta = match dither {
        DitherMode::None => 0,
        DitherMode::Bayer4 => bayer4_value(x, y) as i16 - 8,
    };
    clamp_i16_to_u8(v as i16 + delta * strength)
}

fn bayer4_threshold_u8(x: i32, y: i32) -> u8 {
    (bayer4_value(x, y) << 4) + 8
}

fn bayer4_value(x: i32, y: i32) -> u8 {
    const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

    let xx = x.rem_euclid(4) as usize;
    let yy = y.rem_euclid(4) as usize;
    BAYER4[yy][xx]
}

fn build_tone_lut(curve: ToneCurve) -> [u8; 256] {
    let mut lut = [0u8; 256];
    for i in 0..256 {
        let x = (i as f32) / 255.0;
        let y = match curve {
            ToneCurve::Linear => x,
            // lift paper whites while preserving dark ink pooling
            ToneCurve::Wash => {
                let lifted = x.powf(0.82);
                (0.82 * lifted) + (0.18 * x * x)
            }
            // stronger contrast for edge-first compositions
            ToneCurve::Filmic => {
                let y = (x * (x * 2.51 + 0.03)) / (x * (x * 2.43 + 0.59) + 0.14);
                y.clamp(0.0, 1.0)
            }
            // keep highlights paper-white and compress mids for an ink-wash look
            ToneCurve::SumiE => {
                let ink = x.powf(0.72);
                let dry = x * x * x;
                let y = (ink * 0.62) + (dry * 0.38);
                y.clamp(0.0, 1.0)
            }
        };
        lut[i] = ((y.clamp(0.0, 1.0) * 255.0) + 0.5) as u8;
    }
    lut
}

fn mul8(a: u8, b: u8) -> u8 {
    (((a as u16 * b as u16) + 128) >> 8) as u8
}

fn mix_u8(a: u8, b: u8, t: u8) -> u8 {
    ((((a as u16) * (255 - t) as u16) + ((b as u16) * t as u16) + 128) >> 8) as u8
}

fn clamp_i16_to_u8(v: i16) -> u8 {
    v.clamp(0, 255) as u8
}

fn load_grayscale_resize(path: &Path, width: u16, height: u16) -> Result<Vec<u8>, String> {
    let img = image::open(path)
        .map_err(|e| format!("open ghost image {}: {e}", path.display()))?
        .to_luma8();

    let out = if img.width() == width as u32 && img.height() == height as u32 {
        img
    } else {
        image::imageops::resize(
            &img,
            width as u32,
            height as u32,
            image::imageops::FilterType::CatmullRom,
        )
    };

    Ok(out.into_raw())
}

fn save_gray(path: &Path, width: u16, height: u16, pixels: &[u8]) -> Result<(), String> {
    let img: GrayImage = ImageBuffer::from_vec(width as u32, height as u32, pixels.to_vec())
        .ok_or_else(|| "buffer size mismatch for gray image".to_owned())?;
    img.save(path)
        .map_err(|e| format!("save {}: {e}", path.display()))
}

fn read_bytes<'a>(
    bytes: &'a [u8],
    offset: &mut usize,
    len: usize,
    what: &str,
) -> Result<&'a [u8], String> {
    let start = *offset;
    let end = start + len;
    if end > bytes.len() {
        return Err(format!("unexpected eof while reading {what}"));
    }
    *offset = end;
    Ok(&bytes[start..end])
}

fn read_u16(bytes: &[u8], offset: &mut usize, what: &str) -> Result<u16, String> {
    let raw = read_bytes(bytes, offset, 2, what)?;
    let mut b = [0u8; 2];
    b.copy_from_slice(raw);
    Ok(u16::from_le_bytes(b))
}

fn read_u32(bytes: &[u8], offset: &mut usize, what: &str) -> Result<u32, String> {
    let raw = read_bytes(bytes, offset, 4, what)?;
    let mut b = [0u8; 4];
    b.copy_from_slice(raw);
    Ok(u32::from_le_bytes(b))
}

fn read_u64(bytes: &[u8], offset: &mut usize, what: &str) -> Result<u64, String> {
    let raw = read_bytes(bytes, offset, 8, what)?;
    let mut b = [0u8; 8];
    b.copy_from_slice(raw);
    Ok(u64::from_le_bytes(b))
}

fn mode_name(mode: OutputMode) -> &'static str {
    match mode {
        OutputMode::Mono1 => "mono1",
        OutputMode::Gray3 => "gray3",
        OutputMode::Gray4 => "gray4",
        OutputMode::Gray8 => "gray8",
    }
}

fn apply_preset(cfg: &mut Config, preset: RenderPreset) {
    match preset {
        RenderPreset::SumiE => {
            cfg.mode = OutputMode::Gray3;
            cfg.dither = DitherMode::Bayer4;
            cfg.edge_strength = 148;
            cfg.fog_strength = 98;
            cfg.stroke_strength = 54;
            cfg.paper_strength = 38;
            cfg.tone_curve = ToneCurve::SumiE;
            if cfg.sun_strength == 0 {
                cfg.sun_strength = 136;
            }
        }
    }
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

fn build_depth_relit_map(
    depth: &[u8],
    normal_xy: Option<(&[u8], &[u8])>,
    width: usize,
    height: usize,
    azimuth_deg: f32,
    elevation_deg: f32,
) -> Vec<u8> {
    let mut out = vec![0u8; depth.len()];
    let az = azimuth_deg.to_radians();
    let el = elevation_deg.to_radians().clamp(0.05, 1.5);
    let lx = el.cos() * az.cos();
    let ly = el.cos() * az.sin();
    let lz = el.sin();
    let side_weight = 0.82f32;
    let up_weight = 0.35f32;

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let x0 = x.saturating_sub(1);
            let x1 = (x + 1).min(width - 1);
            let y0 = y.saturating_sub(1);
            let y1 = (y + 1).min(height - 1);

            let (nxn, nyn, nzn) = if let Some((nx_map, ny_map)) = normal_xy {
                let nx = ((nx_map[idx] as f32) - 128.0) / 127.0;
                let ny = ((ny_map[idx] as f32) - 128.0) / 127.0;
                let nz = (1.0 - (nx * nx + ny * ny)).max(0.0).sqrt().max(0.05);
                let inv = (nx * nx + ny * ny + nz * nz).sqrt().recip();
                (nx * inv, ny * inv, nz * inv)
            } else {
                let dx = (depth[y * width + x1] as f32 - depth[y * width + x0] as f32) / 255.0;
                let dy = (depth[y1 * width + x] as f32 - depth[y0 * width + x] as f32) / 255.0;

                // Reconstruct a pseudo normal from depth and amplify lateral slope response.
                // Higher XY gain makes azimuth changes more visible in static-camera relighting.
                let nx = -dx * 11.0;
                let ny = -dy * 11.0;
                let nz = 0.34;

                let inv = (nx * nx + ny * ny + nz * nz).sqrt().recip();
                (nx * inv, ny * inv, nz * inv)
            };

            let side = (nxn * lx) + (nyn * ly);
            let up = nzn * lz;
            let lit = (side * side_weight) + (up * up_weight);
            // Add a subtle global directional ramp so azimuth is readable even on low-detail depth.
            let xf = if width > 1 {
                (x as f32 / (width - 1) as f32) * 2.0 - 1.0
            } else {
                0.0
            };
            let yf = if height > 1 {
                (y as f32 / (height - 1) as f32) * 2.0 - 1.0
            } else {
                0.0
            };
            let directional = (xf * lx) + (yf * ly);
            let shade = (0.62 + lit * 0.72 + directional * 0.28).clamp(0.10, 1.0);
            out[idx] = (shade * 255.0 + 0.5) as u8;
        }
    }

    out
}

fn ink_brush_delta(
    idx: usize,
    x: usize,
    y: usize,
    stroke: u8,
    edge: u8,
    depth: u8,
    normal_xy: Option<(&[u8], &[u8])>,
    stroke_strength: u8,
) -> i16 {
    if stroke_strength == 0 {
        return 0;
    }

    let xf = x as f32;
    let yf = y as f32;
    let depth_f = (depth as f32) / 255.0;
    let edge_f = (edge as f32) / 255.0;
    let stroke_src = ((stroke as f32) - 128.0) / 127.0;

    let (tx, ty, nx, ny) = brush_basis(idx, normal_xy);

    // Vary stroke spacing with depth and contour strength.
    let freq_macro = 0.007 + (0.004 * (1.0 - depth_f));
    let freq_coarse = 0.016 + (0.012 * (1.0 - depth_f));
    let freq_fine = 0.058 + (0.034 * edge_f);

    let u = (xf * tx) + (yf * ty);
    let v = (xf * nx) + (yf * ny);

    let phase0 = hash01((x as i32) >> 4, (y as i32) >> 4, 0xA1B2_C3D4) * core::f32::consts::TAU;
    let phase1 = hash01((x as i32) >> 5, (y as i32) >> 5, 0x9E37_79B1) * core::f32::consts::TAU;
    let phase2 = hash01((x as i32) >> 6, (y as i32) >> 6, 0x7F4A_7C15) * core::f32::consts::TAU;
    let phase3 = hash01((x as i32) >> 7, (y as i32) >> 7, 0xC6A4_A793) * core::f32::consts::TAU;

    let line_macro = (u * freq_macro + phase2).sin();
    let line_coarse = (u * freq_coarse + phase0).sin();
    let line_fine = ((u * freq_fine) + (v * 0.011) + phase1).sin();
    let cross_wash = ((u * (freq_macro * 0.8) + phase2).sin())
        * ((v * (freq_macro * 0.55) + phase3).cos());

    // Low-frequency patchiness prevents uniformly repeated texture.
    let patch = (hash01((x as i32) >> 5, (y as i32) >> 5, 0x85EB_CA77) * 2.0) - 1.0;
    let micro = (hash01(x as i32, y as i32, 0xC2B2_AE3D) * 2.0) - 1.0;

    let brush_mix = (line_macro * 0.24)
        + (line_coarse * 0.26)
        + (line_fine * 0.20)
        + (cross_wash * 0.18)
        + (patch * 0.08)
        + (micro * 0.04);
    let signal = ((stroke_src * 0.48) + (brush_mix * 0.52)).clamp(-1.0, 1.0);

    // Deliberately stylized regime: allow visible brush dominance that can bend silhouette perception.
    let strength_f = (stroke_strength as f32) / 255.0;
    let chaos_boost = 1.0 + (2.8 * strength_f.powf(1.05));
    let amp = (stroke_strength as f32)
        * (0.82 + (1.35 * edge_f) + (0.70 * (1.0 - depth_f)))
        * chaos_boost;
    let delta = (signal * amp).round() as i16;
    delta.clamp(-208, 208)
}

fn brush_basis(idx: usize, normal_xy: Option<(&[u8], &[u8])>) -> (f32, f32, f32, f32) {
    if let Some((nx_map, ny_map)) = normal_xy {
        let nx = ((nx_map[idx] as f32) - 128.0) / 127.0;
        let ny = ((ny_map[idx] as f32) - 128.0) / 127.0;
        let nlen = (nx * nx + ny * ny).sqrt();
        if nlen > 0.03 {
            let nnx = nx / nlen;
            let nny = ny / nlen;
            // Tangent is perpendicular to normal.
            return (-nny, nnx, nnx, nny);
        }
    }

    // Fallback orientation if normals are unavailable.
    (1.0, 0.0, 0.0, 1.0)
}

fn hash01(x: i32, y: i32, seed: u32) -> f32 {
    let mut v = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (y as u32).wrapping_mul(0x85EB_CA77)
        ^ seed;
    v ^= v >> 16;
    v = v.wrapping_mul(0x7FEB_352D);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846C_A68B);
    v ^= v >> 16;
    (v as f32) / (u32::MAX as f32)
}

fn paper_noise_u8(x: i32, y: i32) -> u8 {
    let mut v = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    v ^= v >> 15;
    v = v.wrapping_mul(0xC2B2_AE3D);
    v ^= v >> 13;
    (v & 0xFF) as u8
}

fn print_help() {
    println!(
        "scene_viewer\n\n
actions:\n  render   Render a .scenebundle into a grayscale PNG using device-like compositing\n  inspect  Print bundle summary\n\n
action: render\n  --bundle FILE           Input bundle (default: tools/scene_maker/out/scene.scenebundle)\n  --out FILE              Output PNG (default: tools/scene_viewer/out/render.png)\n  --mode MODE             mono1|gray3|gray4|gray8 (default: gray3)\n  --dither MODE           none|bayer4 (default: bayer4)\n  --edge-strength N       0..255 (default: 96)\n  --fog-strength N        0..255 (default: 72)\n  --stroke-strength N     0..255 (default: 24)\n  --tone-curve MODE       linear|wash|filmic (default: wash)\n  --save-debug DIR        Save intermediates (tone base / stylized / quantized)\n  --dump-channels DIR     Save decoded source channels\n  --ghost-from FILE       Prior rendered frame for ghosting simulation\n  --ghost-alpha N         0..255 blend amount from prior frame (default: 0)\n\n
action: inspect\n  --bundle FILE           Bundle path"
    );
}
