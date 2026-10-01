use std::env;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process;

use hourglass::model::PHYSICS_HZ;
use hourglass::paper_cellular::{
    PaperCellular, DEFAULT_REPOSE_RUN_CELLS, DEFAULT_SURFACE_RELAX_INTERVAL,
    DEFAULT_TOPPLE_PER_MILLE, MAX_REPOSE_RUN_CELLS, MAX_SURFACE_RELAX_INTERVAL, MEDINOTE_GRAINS,
    TOPPLE_SCALE,
};
use image::codecs::gif::{GifEncoder, Repeat};
use image::imageops::FilterType;
use image::{Delay, Frame, GrayImage, ImageBuffer, Luma, RgbaImage};

const WIDTH: u32 = 140;
const HEIGHT: u32 = 200;
const TIME_LABEL_HEIGHT: u32 = 9;
const OUTPUT_HEIGHT: u32 = HEIGHT + TIME_LABEL_HEIGHT;
const DEFAULT_SECONDS: u32 = 20;
const DEFAULT_SCALE: u32 = 3;
const DEFAULT_GIF_FPS: u32 = 20;
const DEFAULT_GIF_STEP_TICKS: u32 = 2;
const DEFAULT_SEED: u32 = 1;
const PAPER_REFERENCE_GRAINS: usize = 500;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Geometry {
    Medinote,
    Paper,
}

impl Geometry {
    const fn name(self) -> &'static str {
        match self {
            Self::Medinote => "medinote",
            Self::Paper => "paper",
        }
    }
}

struct Config {
    out: PathBuf,
    geometry: Geometry,
    seconds: u32,
    scale: u32,
    gif_fps: u32,
    gif_step_ticks: u32,
    png_seconds: Vec<u32>,
    grains: usize,
    topple_per_mille: u16,
    repose_run_cells: u8,
    surface_relax_interval: u8,
    seed: u32,
}

fn usage() -> ! {
    eprintln!(
        "usage: cargo run -p hourglass --release --example hourglass_paper_preview -- [OPTIONS]\n\
         \n\
         Options:\n\
           --out DIR                  Output directory (default: logs/hourglass_paper_preview)\n\
           --geometry NAME            medinote or paper (default: medinote)\n\
           --seconds N                Simulated on-device seconds (default: {DEFAULT_SECONDS})\n\
           --scale N                  Nearest-neighbour output scale (default: {DEFAULT_SCALE})\n\
           --gif-fps N                Playback frames per second (default: {DEFAULT_GIF_FPS})\n\
           --gif-step-ticks N         Model ticks per GIF frame (default: {DEFAULT_GIF_STEP_TICKS})\n\
           --png-at S0,S1,...         Snapshot times in simulated seconds\n\
           --grains N                 Visible sand cells (default: geometry-specific)\n\
           --probability N            Topple probability per mille, 0..1000 (default: 750)\n\
           --repose-run N             Surface run, 0..4; 0 disables (Medinote default: {DEFAULT_REPOSE_RUN_CELLS})\n\
           --repose-every N           Surface pass interval, 1..32 (Medinote default: 4)\n\
           --seed N                   Deterministic random seed (default: {DEFAULT_SEED})\n\
           -h, --help                 Show this help\n\
         \n\
         Output contains sand cells and elapsed on-device time only."
    );
    process::exit(2);
}

fn parse_positive(value: Option<String>) -> u32 {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| usage())
}

fn parse_usize(value: Option<String>) -> usize {
    value
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| usage())
}

fn parse_probability(value: Option<String>) -> u16 {
    value
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value <= TOPPLE_SCALE)
        .unwrap_or_else(|| usage())
}

fn parse_repose_run(value: Option<String>) -> u8 {
    value
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|value| *value <= MAX_REPOSE_RUN_CELLS)
        .unwrap_or_else(|| usage())
}

fn parse_repose_interval(value: Option<String>) -> u8 {
    value
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|value| (1..=MAX_SURFACE_RELAX_INTERVAL).contains(value))
        .unwrap_or_else(|| usage())
}

fn parse_geometry(value: Option<String>) -> Geometry {
    match value.as_deref() {
        Some("medinote") => Geometry::Medinote,
        Some("paper") => Geometry::Paper,
        _ => usage(),
    }
}

fn parse_png_seconds(value: Option<String>, total_seconds: u32) -> Vec<u32> {
    let mut seconds = value
        .unwrap_or_else(|| usage())
        .split(',')
        .map(|part| part.parse::<u32>().unwrap_or_else(|_| usage()))
        .filter(|second| *second <= total_seconds)
        .collect::<Vec<_>>();
    seconds.sort_unstable();
    seconds.dedup();
    seconds
}

fn default_png_seconds(total_seconds: u32) -> Vec<u32> {
    let mut seconds = [0, 1, 2, 5, 10, total_seconds / 2, total_seconds]
        .into_iter()
        .filter(|second| *second <= total_seconds)
        .collect::<Vec<_>>();
    seconds.sort_unstable();
    seconds.dedup();
    seconds
}

fn parse_config() -> Config {
    let mut out = PathBuf::from("logs/hourglass_paper_preview");
    let mut geometry = Geometry::Medinote;
    let mut seconds = DEFAULT_SECONDS;
    let mut scale = DEFAULT_SCALE;
    let mut gif_fps = DEFAULT_GIF_FPS;
    let mut gif_step_ticks = DEFAULT_GIF_STEP_TICKS;
    let mut png_at = None;
    let mut grains = None;
    let mut topple_per_mille = DEFAULT_TOPPLE_PER_MILLE;
    let mut repose_run_cells = None;
    let mut surface_relax_interval = None;
    let mut seed = DEFAULT_SEED;
    let mut arguments = env::args().skip(1);

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--out" => out = PathBuf::from(arguments.next().unwrap_or_else(|| usage())),
            "--geometry" => geometry = parse_geometry(arguments.next()),
            "--seconds" => seconds = parse_positive(arguments.next()),
            "--scale" => scale = parse_positive(arguments.next()),
            "--gif-fps" => gif_fps = parse_positive(arguments.next()),
            "--gif-step-ticks" => gif_step_ticks = parse_positive(arguments.next()),
            "--png-at" => png_at = Some(arguments.next().unwrap_or_else(|| usage())),
            "--grains" => grains = Some(parse_usize(arguments.next())),
            "--probability" => topple_per_mille = parse_probability(arguments.next()),
            "--repose-run" => repose_run_cells = Some(parse_repose_run(arguments.next())),
            "--repose-every" => {
                surface_relax_interval = Some(parse_repose_interval(arguments.next()));
            }
            "--seed" => {
                seed = arguments
                    .next()
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or_else(|| usage());
            }
            "-h" | "--help" => usage(),
            _ => usage(),
        }
    }

    let png_seconds = png_at
        .map(|value| parse_png_seconds(Some(value), seconds))
        .unwrap_or_else(|| default_png_seconds(seconds));
    let grains = grains.unwrap_or(match geometry {
        Geometry::Medinote => MEDINOTE_GRAINS,
        Geometry::Paper => PAPER_REFERENCE_GRAINS,
    });
    let repose_run_cells = repose_run_cells.unwrap_or(match geometry {
        Geometry::Medinote => DEFAULT_REPOSE_RUN_CELLS,
        Geometry::Paper => 0,
    });
    let surface_relax_interval = surface_relax_interval.unwrap_or(match geometry {
        Geometry::Medinote => DEFAULT_SURFACE_RELAX_INTERVAL,
        Geometry::Paper => 1,
    });
    Config {
        out,
        geometry,
        seconds,
        scale,
        gif_fps,
        gif_step_ticks,
        png_seconds,
        grains,
        topple_per_mille,
        repose_run_cells,
        surface_relax_interval,
        seed,
    }
}

fn build_model(config: &Config) -> Result<PaperCellular, String> {
    let model = match config.geometry {
        Geometry::Medinote => PaperCellular::medinote_unpaced_with_surface(
            config.grains,
            config.topple_per_mille,
            config.seed,
            config.repose_run_cells,
            config.surface_relax_interval,
        ),
        Geometry::Paper if config.repose_run_cells == 0 => {
            PaperCellular::paper_reference(config.grains, config.topple_per_mille, config.seed)
        }
        Geometry::Paper => None,
    };
    model.ok_or_else(|| {
        format!(
            "invalid {} setup: grains={} probability={} repose_run={} repose_every={}",
            config.geometry.name(),
            config.grains,
            config.topple_per_mille,
            config.repose_run_cells,
            config.surface_relax_interval,
        )
    })
}

fn render_cells(model: &PaperCellular, scale: u32, device_tick: u32) -> GrayImage {
    let mut native = ImageBuffer::from_pixel(WIDTH, OUTPUT_HEIGHT, Luma([0xFF]));
    for cell in model.sand_cells() {
        let x = i32::from(cell.x) + WIDTH as i32 / 2;
        let y = i32::from(cell.y) + HEIGHT as i32 / 2;
        if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) {
            native.put_pixel(x as u32, y as u32, Luma([0x00]));
        }
    }
    draw_time_label(&mut native, device_tick);
    if scale == 1 {
        native
    } else {
        image::imageops::resize(
            &native,
            WIDTH * scale,
            OUTPUT_HEIGHT * scale,
            FilterType::Nearest,
        )
    }
}

fn draw_time_label(image: &mut GrayImage, device_tick: u32) {
    let total_seconds = device_tick / PHYSICS_HZ;
    let label = format!("t={:02}:{:02}", total_seconds / 60, total_seconds % 60);
    let mut x = 2;
    for character in label.bytes() {
        draw_glyph(image, x, HEIGHT + 2, glyph(character));
        x += 4;
    }
}

fn draw_glyph(image: &mut GrayImage, x: u32, y: u32, rows: [u8; 5]) {
    for (row, bits) in rows.into_iter().enumerate() {
        for column in 0..3 {
            if bits & (1 << (2 - column)) != 0 {
                image.put_pixel(x + column, y + row as u32, Luma([0x00]));
            }
        }
    }
}

const fn glyph(character: u8) -> [u8; 5] {
    match character {
        b'0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        b'2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        b'3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        b'4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        b'5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        b'7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        b'8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        b'9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        b't' => [0b010, 0b111, 0b010, 0b010, 0b011],
        b'=' => [0b000, 0b111, 0b000, 0b111, 0b000],
        b':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        _ => [0; 5],
    }
}

fn encode_gif_frame(
    encoder: &mut GifEncoder<BufWriter<File>>,
    image: GrayImage,
    gif_fps: u32,
) -> image::ImageResult<()> {
    let rgba = RgbaImage::from_fn(image.width(), image.height(), |x, y| {
        let value = image.get_pixel(x, y).0[0];
        image::Rgba([value, value, value, 0xFF])
    });
    let delay = Delay::from_numer_denom_ms(1_000, gif_fps);
    encoder.encode_frame(Frame::from_parts(rgba, 0, 0, delay))
}

fn save_png(out: &Path, second: u32, image: &GrayImage) -> image::ImageResult<()> {
    image.save(out.join(format!("paper_cells_{second:04}s.png")))
}

fn run(config: Config) -> Result<(), String> {
    fs::create_dir_all(&config.out)
        .map_err(|error| format!("create {}: {error}", config.out.display()))?;
    let gif_path = config.out.join("paper_cells.gif");
    let gif_file = File::create(&gif_path)
        .map_err(|error| format!("create {}: {error}", gif_path.display()))?;
    let mut gif = GifEncoder::new_with_speed(BufWriter::new(gif_file), 10);
    gif.set_repeat(Repeat::Infinite)
        .map_err(|error| format!("configure {}: {error}", gif_path.display()))?;

    let mut model = build_model(&config)?;
    let total_ticks = config
        .seconds
        .checked_mul(PHYSICS_HZ)
        .ok_or_else(|| "preview duration overflows tick count".to_owned())?;
    let mut next_png = 0;

    for tick in 0..=total_ticks {
        let second = tick / PHYSICS_HZ;
        let gif_frame = tick % config.gif_step_ticks == 0 || tick == total_ticks;
        let png_frame = next_png < config.png_seconds.len()
            && tick == config.png_seconds[next_png].saturating_mul(PHYSICS_HZ);
        if gif_frame || png_frame {
            let image = render_cells(&model, config.scale, tick);
            if gif_frame {
                encode_gif_frame(&mut gif, image.clone(), config.gif_fps)
                    .map_err(|error| format!("encode {}: {error}", gif_path.display()))?;
            }
            if png_frame {
                save_png(&config.out, second, &image).map_err(|error| {
                    format!("save PNG at {second}s in {}: {error}", config.out.display())
                })?;
                next_png += 1;
            }
        }
        if tick < total_ticks {
            model.step_down();
        }
    }

    println!(
        "paper hourglass preview complete: geometry={} simulated={}s generations={} grains={} probability={} repose_run={} repose_every={} seed={} gif={} pngs={} output={} resolution={}x{}",
        config.geometry.name(),
        config.seconds,
        model.generation(),
        config.grains,
        config.topple_per_mille,
        config.repose_run_cells,
        config.surface_relax_interval,
        config.seed,
        gif_path.display(),
        config.png_seconds.len(),
        config.out.display(),
        WIDTH * config.scale,
        OUTPUT_HEIGHT * config.scale,
    );
    Ok(())
}

fn main() {
    if let Err(error) = run(parse_config()) {
        eprintln!("paper hourglass preview failed: {error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_keeps_time_below_sand_area() {
        let model = PaperCellular::paper_reference(500, DEFAULT_TOPPLE_PER_MILLE, 1).unwrap();
        let start = render_cells(&model, 1, 0);
        let later = render_cells(&model, 1, PHYSICS_HZ * 60);
        assert_eq!(start.dimensions(), (WIDTH, OUTPUT_HEIGHT));
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                assert_eq!(start.get_pixel(x, y), later.get_pixel(x, y));
            }
        }
        assert!((HEIGHT..OUTPUT_HEIGHT)
            .any(|y| (0..WIDTH).any(|x| start.get_pixel(x, y) != later.get_pixel(x, y))));
    }

    #[test]
    fn default_snapshots_include_start_and_end() {
        let seconds = default_png_seconds(DEFAULT_SECONDS);
        assert_eq!(seconds.first(), Some(&0));
        assert_eq!(seconds.last(), Some(&DEFAULT_SECONDS));
    }
}
