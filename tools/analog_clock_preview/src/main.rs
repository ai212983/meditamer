//! `analog_clock_preview`: render, benchmark, and serve the shared
//! analog-clock CPU renderer.
//!
//! ```text
//! analog_clock_preview render --out-dir .scratch/analog-clock [--time 10:09]
//! analog_clock_preview bench [--frames 5]
//! analog_clock_preview serve [--port 8901]
//! analog_clock_preview export-maps --out-dir .scratch/analog-clock-maps
//! ```
//!
//! Asset resolution is manifest-relative (`../../assets` beside the tool)
//! unless `--assets` overrides it; no absolute paths are baked in.

mod assets;
mod comparison;
mod composition;
mod frame;
mod http;
mod map_export;
mod params;

use std::path::PathBuf;

use params::{OutputMode, PreviewParams};

pub(crate) fn usage() -> &'static str {
    "usage:\n  analog_clock_preview render --out-dir DIR [--time HH:MM] [--assets DIR] [--dither ALG] [--background-dither ALG] [--clock-dither ALG] [--hands-dither ALG] [--shadows-dither ALG] [--hand-darkness 0..1] [--composition MODE] [--compare-time HH:MM] [--diagnostics 0|1]\n    ALG is none|gradient|bayer4|bayer8|blue-noise|floyd-steinberg|atkinson; --dither sets all four regions\n    MODE is legacy|reference|replacement|removal\n  analog_clock_preview bench [--frames N] [--assets DIR] [--composition MODE]\n  analog_clock_preview serve [--port PORT] [--assets DIR]
  analog_clock_preview export-maps --out-dir DIR [--assets DIR]"
}

/// Every flag `render` accepts. `--dither` is the legacy global (sets all
/// four regions); the four `--*-dither` flags tune one region each.
/// `--composition` picks the host composition experiment, `--compare-time`
/// the reference time, `--diagnostics` the `0|1` comparison headers.
const RENDER_FLAGS: &[&str] = &[
    "--out-dir",
    "--time",
    "--assets",
    "--dither",
    "--background-dither",
    "--clock-dither",
    "--hands-dither",
    "--shadows-dither",
    "--hand-darkness",
    "--composition",
    "--compare-time",
    "--diagnostics",
];

pub(crate) fn flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(String::as_str).unwrap_or("");
    let assets_dir = flag(&args, "--assets")
        .map(PathBuf::from)
        .unwrap_or_else(assets::default_asset_dir);
    let result = match command {
        "render" => cmd_render(&args, &assets_dir),
        "bench" => cmd_bench(&args, &assets_dir),
        "serve" => cmd_serve(&args, &assets_dir),
        "export-maps" => map_export::cmd_export_maps(&args, &assets_dir),
        _ => Err(usage().to_string()),
    };
    if let Err(message) = result {
        if message.starts_with("usage:") {
            eprintln!("{message}");
        } else {
            eprintln!("error: {message}");
        }
        std::process::exit(1);
    }
}

/// Rejects unrecognized `--*` flags so typos surface instead of silently
/// rendering defaults. `known` lists every flag the command accepts.
pub(crate) fn reject_unknown_flags(args: &[String], known: &[&str]) -> Result<(), String> {
    for arg in &args[2..] {
        if let Some(name) = arg.strip_prefix("--") {
            let name = name.split_once('=').map_or(name, |(k, _)| k);
            let flag = format!("--{name}");
            if !known.iter().any(|k| *k == flag) {
                return Err(format!("unknown flag {flag}"));
            }
        }
    }
    Ok(())
}

pub(crate) fn load_assets(dir: &std::path::Path) -> Result<assets::DecodedHands, String> {
    let hands = assets::load_hands(dir)?;
    if !hands.as_hands().validate() {
        return Err("decoded assets failed validation".to_string());
    }
    Ok(hands)
}

/// Exports deterministic snapshots: grayscale and dithered PNGs at
/// 600x600 and 400x300, all through the shared core. Source assets stay
/// unchanged; these are host-side exports only.
fn cmd_render(args: &[String], assets_dir: &std::path::Path) -> Result<(), String> {
    reject_unknown_flags(args, RENDER_FLAGS)?;
    let out_dir = flag(args, "--out-dir").ok_or_else(|| usage().to_string())?;
    let hands = load_assets(assets_dir)?;
    // The one attach point for CLI snapshots: every size below clones
    // `base`, so legacy, gray, and composed views share the supplied image.
    let mut base = PreviewParams {
        dial: hands.dial.clone(),
        ..PreviewParams::default()
    };
    if let Some(time) = flag(args, "--time") {
        base.apply("time", &time)?;
    }
    if let Some(dither) = flag(args, "--dither") {
        base.apply("dither", &dither)?;
        base.mode = OutputMode::Dithered;
    }
    for (cli_flag, key) in [
        ("--background-dither", "background_dither"),
        ("--clock-dither", "clock_dither"),
        ("--hands-dither", "hands_dither"),
        ("--shadows-dither", "shadows_dither"),
    ] {
        if let Some(raw) = flag(args, cli_flag) {
            base.apply(key, &raw)?;
            base.mode = OutputMode::Dithered;
        }
    }
    for (cli_flag, key) in [
        ("--hand-darkness", "hand_darkness"),
        ("--composition", "composition"),
        ("--compare-time", "compare_time"),
        ("--diagnostics", "diagnostics"),
    ] {
        if let Some(raw) = flag(args, cli_flag) {
            base.apply(key, &raw)?;
        }
    }
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("cannot create {out_dir}: {e}"))?;
    // One host-owned cache for both snapshots: a repeated static base is
    // an exact byte hit, any base input change replaces the entry.
    let mut cache = frame::FrameCache::new();
    for (w, h) in [(600u32, 600u32), (400u32, 300u32)] {
        let mut gray = base.clone();
        gray.apply("w", &w.to_string())?;
        gray.apply("h", &h.to_string())?;
        gray.mode = OutputMode::Gray;
        let buf = params::render_gray(&gray, &params::hands(&gray, &hands));
        let path = format!("{out_dir}/clock-{w}x{h}-gray.png");
        write_png(&path, w, h, &buf)?;
        println!("wrote {path}");
        let mut dithered = base.clone();
        dithered.apply("w", &w.to_string())?;
        dithered.apply("h", &h.to_string())?;
        dithered.mode = OutputMode::Dithered;
        let borrowed = params::hands(&dithered, &hands);
        let out = frame::render_frame(&mut cache, &dithered, &borrowed)?;
        let buf = params::bits_to_gray(&out.bits, w as usize * h as usize);
        let path = format!("{out_dir}/clock-{w}x{h}-dither.png");
        write_png(&path, w, h, &buf)?;
        println!("wrote {path}");
    }
    Ok(())
}

fn write_png(path: &str, w: u32, h: u32, gray: &[u8]) -> Result<(), String> {
    image::save_buffer(path, gray, w, h, image::ExtendedColorType::L8)
        .map_err(|e| format!("cannot write {path}: {e}"))
}

/// Host rendering timings (labeled HOST, never device numbers): mean and
/// range over `--frames` full 600x600 grayscale frames.
fn cmd_bench(args: &[String], assets_dir: &std::path::Path) -> Result<(), String> {
    reject_unknown_flags(
        args,
        &["--frames", "--assets", "--composition", "--hand-darkness"],
    )?;
    let frames: usize = flag(args, "--frames")
        .map(|raw| {
            raw.parse()
                .map_err(|_| "--frames must be a number".to_string())
        })
        .transpose()?
        .unwrap_or(5)
        .max(1);
    let hands = load_assets(assets_dir)?;
    let mut params = PreviewParams {
        dial: hands.dial.clone(),
        ..PreviewParams::default()
    };
    if let Some(raw) = flag(args, "--composition") {
        params.apply("composition", &raw)?;
    }
    if let Some(raw) = flag(args, "--hand-darkness") {
        params.apply("hand_darkness", &raw)?;
    }
    params.mode = OutputMode::Dithered;
    let borrowed = params::hands(&params, &hands);
    let mut cache = frame::FrameCache::new();
    let mut times = Vec::with_capacity(frames);
    for _ in 0..frames {
        let started = std::time::Instant::now();
        let out = frame::render_frame(&mut cache, &params, &borrowed)?;
        std::hint::black_box(out.bits);
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    println!(
        "HOST render 600x600 dithered ({}), {} frames: mean {mean:.1} ms, min {:.1} ms, max {:.1} ms (host only, not device numbers)",
        params.composition.as_str(),
        times.len(),
        times[0],
        times[times.len() - 1]
    );
    Ok(())
}

fn cmd_serve(args: &[String], assets_dir: &std::path::Path) -> Result<(), String> {
    reject_unknown_flags(args, &["--port", "--assets"])?;
    let port: u16 = flag(args, "--port")
        .map(|raw| {
            raw.parse()
                .map_err(|_| "--port must be a number".to_string())
        })
        .transpose()?
        .unwrap_or(8901);
    let hands = load_assets(assets_dir)?;
    http::serve(port, hands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn regional_flags_are_known_and_unknown_still_errors() {
        for extra in [
            "--dither",
            "--background-dither",
            "--clock-dither",
            "--hands-dither",
            "--shadows-dither",
        ] {
            let with_flag = args(&[
                "analog_clock_preview",
                "render",
                "--out-dir",
                "x",
                extra,
                "gradient",
            ]);
            assert!(
                reject_unknown_flags(&with_flag, RENDER_FLAGS).is_ok(),
                "{extra} should be accepted"
            );
        }
        let unknown = args(&[
            "analog_clock_preview",
            "render",
            "--out-dir",
            "x",
            "--ditheer",
            "gradient",
        ]);
        assert!(reject_unknown_flags(&unknown, RENDER_FLAGS).is_err());
        let unknown_regional = args(&[
            "analog_clock_preview",
            "render",
            "--out-dir",
            "x",
            "--hand-dither",
            "none",
        ]);
        assert!(reject_unknown_flags(&unknown_regional, RENDER_FLAGS).is_err());
    }

    #[test]
    fn hand_darkness_flag_is_known_and_reaches_params() {
        let argv = args(&[
            "analog_clock_preview",
            "render",
            "--out-dir",
            "x",
            "--hand-darkness",
            "0.5",
        ]);
        assert!(reject_unknown_flags(&argv, RENDER_FLAGS).is_ok());
        let raw = flag(&argv, "--hand-darkness").expect("flag present");
        let mut base = PreviewParams::default();
        base.apply("hand_darkness", &raw).expect("valid darkness");
        assert!((base.hand_darkness - 0.5).abs() < 1e-6);
        let bench = args(&["analog_clock_preview", "bench", "--hand-darkness", "0.5"]);
        assert!(reject_unknown_flags(
            &bench,
            &["--frames", "--assets", "--composition", "--hand-darkness"]
        )
        .is_ok());
    }

    #[test]
    fn regional_flag_values_reach_params() {
        // Same mapping the render command uses: kebab flag, snake key.
        let mut base = PreviewParams::default();
        for (cli_flag, key) in [
            ("--background-dither", "background_dither"),
            ("--clock-dither", "clock_dither"),
            ("--hands-dither", "hands_dither"),
            ("--shadows-dither", "shadows_dither"),
        ] {
            let argv = args(&["analog_clock_preview", "render", cli_flag, "atkinson"]);
            let raw = flag(&argv, cli_flag).expect("flag present");
            base.apply(key, &raw).expect("valid method");
        }
        assert!([
            base.regions.background,
            base.regions.clock,
            base.regions.hands,
            base.regions.shadows
        ]
        .iter()
        .all(|m| *m == analog_clock::DitherAlgorithm::Atkinson));
    }
}
