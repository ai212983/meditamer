use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

#[allow(dead_code)]
#[path = "../../../src/shanshui.rs"]
mod shanshui;

#[derive(Clone, Copy)]
enum PreviewMode {
    Legacy,
    Atkinson,
}

struct Config {
    out_dir: PathBuf,
    width: i32,
    height: i32,
    seed_start: u32,
    count: u32,
    mode: PreviewMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::from("tools/shanshui_preview/out"),
            width: 600,
            height: 600,
            seed_start: 12_345,
            count: 4,
            mode: PreviewMode::Atkinson,
        }
    }
}

fn main() -> Result<(), String> {
    let cfg = parse_args(env::args().skip(1))?;
    if cfg.width <= 0 || cfg.height <= 0 {
        return Err("width/height must be > 0".to_owned());
    }
    fs::create_dir_all(&cfg.out_dir).map_err(|e| format!("create output dir: {e}"))?;

    let width = cfg.width as u32;
    let height = cfg.height as u32;
    let pixels = (width as usize) * (height as usize);

    for i in 0..cfg.count {
        let seed = cfg.seed_start.wrapping_add(i);
        let mut buf = vec![255u8; pixels];
        render_seed(&cfg, seed, &mut buf);
        let mode_name = match cfg.mode {
            PreviewMode::Legacy => "legacy",
            PreviewMode::Atkinson => "atkinson",
        };
        let filename = format!("shanshui_{mode_name}_seed_{seed}.png");
        let path = cfg.out_dir.join(filename);
        image::save_buffer(&path, &buf, width, height, image::ColorType::L8)
            .map_err(|e| format!("save {}: {e}", path.display()))?;
        println!("wrote {}", path.display());
    }

    Ok(())
}

fn render_seed(cfg: &Config, seed: u32, buf: &mut [u8]) {
    let w = cfg.width as usize;
    match cfg.mode {
        PreviewMode::Legacy => {
            shanshui::render_shanshui_rows_bw(cfg.width, cfg.height, 0, cfg.height, seed, |x, y| {
                buf[(y as usize) * w + (x as usize)] = 0;
            });
        }
        PreviewMode::Atkinson => {
            shanshui::render_shanshui_bw_atkinson(cfg.width, cfg.height, seed, |x, y| {
                buf[(y as usize) * w + (x as usize)] = 0;
            });
        }
    }
}

fn parse_args<I>(args: I) -> Result<Config, String>
where
    I: IntoIterator<Item = String>,
{
    let mut cfg = Config::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--out" => cfg.out_dir = PathBuf::from(next_value("--out", &mut it)?),
            "--width" => cfg.width = parse_num(next_value("--width", &mut it)?, "--width")?,
            "--height" => cfg.height = parse_num(next_value("--height", &mut it)?, "--height")?,
            "--seed" => cfg.seed_start = parse_num(next_value("--seed", &mut it)?, "--seed")?,
            "--count" => cfg.count = parse_num(next_value("--count", &mut it)?, "--count")?,
            "--mode" => {
                let raw = next_value("--mode", &mut it)?;
                cfg.mode = match raw.as_str() {
                    "legacy" => PreviewMode::Legacy,
                    "atkinson" => PreviewMode::Atkinson,
                    _ => return Err(format!("--mode must be legacy|atkinson, got {raw}")),
                };
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("unknown arg: {arg}")),
        }
    }
    Ok(cfg)
}

fn next_value<I>(flag: &str, it: &mut I) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    it.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_num<T>(raw: String, name: &str) -> Result<T, String>
where
    T: core::str::FromStr,
{
    raw.parse::<T>()
        .map_err(|_| format!("invalid numeric value for {name}: {raw}"))
}

fn print_help() {
    let exe = env::args()
        .next()
        .and_then(|p| {
            Path::new(&p)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_owned())
        })
        .unwrap_or_else(|| "shanshui_preview".to_owned());
    println!(
        "Usage: {exe} [--out DIR] [--seed N] [--count N] [--width W] [--height H] [--mode legacy|atkinson]"
    );
}
