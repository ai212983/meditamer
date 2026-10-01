use std::env;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process;

use hourglass::backend::PARTICLE_CAPACITY;
use hourglass::fixed::Fx;
use hourglass::model::{HourglassModel, DEFAULT_DURATION_S, PHYSICS_HZ};
use hourglass::paper_backend::GRAVITY_SCHEDULE;
use hourglass::raster::CellProjection;
use hourglass::rotation::{RotationCommand, RotationCommandKind, TimestampUs};
use image::codecs::gif::{GifEncoder, Repeat};
use image::imageops::FilterType;
use image::{Delay, Frame, GrayImage, ImageBuffer, Luma, RgbaImage};

const WIDTH: u32 = 140;
const HEIGHT: u32 = 200;
const TIME_LABEL_HEIGHT: u32 = 9;
const OUTPUT_HEIGHT: u32 = HEIGHT + TIME_LABEL_HEIGHT;
const DEFAULT_SCALE: u32 = 3;
const DEFAULT_GIF_FPS: u32 = 30;
const DEFAULT_GIF_STEP_TICKS: u32 = PHYSICS_HZ;

struct Config {
    out: PathBuf,
    seconds: u32,
    scale: u32,
    gif_fps: u32,
    gif_step_ticks: u32,
    png_ticks: Vec<u32>,
    rotations: Vec<ScheduledRotation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScheduledRotation {
    tick: u32,
    degrees: i32,
}

fn usage() -> ! {
    eprintln!(
        "usage: cargo run -p hourglass --release --example hourglass_preview -- [OPTIONS]\n\
         \n\
         Options:\n\
           --out DIR                  Output directory (default: logs/hourglass_preview)\n\
           --seconds N                Simulated seconds (default: full calibrated duration)\n\
           --scale N                  Nearest-neighbour output scale (default: 3)\n\
           --gif-fps N                Playback frames per second (default: 30)\n\
           --gif-step-seconds N       Simulated seconds per GIF frame (default: 1)\n\
           --gif-step-ticks N         Simulated physics ticks per GIF frame (default: 30)\n\
           --png-at S0,S1,...         Snapshot times in simulated seconds\n\
           --rotate T:D,...           Rotate by signed integer degrees at tick T\n\
           -h, --help                 Show this help\n\
         \n\
         Output contains particle cells and elapsed on-device time: no glass or tracers."
    );
    process::exit(2);
}

fn parse_positive(value: Option<String>) -> u32 {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| usage())
}

fn parse_png_ticks(value: Option<String>, total_ticks: u32) -> Vec<u32> {
    let value = value.unwrap_or_else(|| usage());
    let mut ticks = value
        .split(',')
        .map(|part| part.parse::<u32>().unwrap_or_else(|_| usage()))
        .map(|second| second.checked_mul(PHYSICS_HZ).unwrap_or_else(|| usage()))
        .filter(|tick| *tick <= total_ticks)
        .collect::<Vec<_>>();
    ticks.sort_unstable();
    ticks.dedup();
    ticks
}

fn default_png_ticks(total_ticks: u32) -> Vec<u32> {
    let candidates = [
        0,
        1,
        30 * PHYSICS_HZ,
        100 * PHYSICS_HZ,
        total_ticks / 4,
        total_ticks / 2,
        total_ticks.saturating_mul(3) / 4,
        total_ticks.saturating_sub(120 * PHYSICS_HZ),
        total_ticks.saturating_sub(90 * PHYSICS_HZ),
        total_ticks.saturating_sub(60 * PHYSICS_HZ),
        total_ticks.saturating_sub(45 * PHYSICS_HZ),
        total_ticks.saturating_sub(30 * PHYSICS_HZ),
        total_ticks.saturating_sub(20 * PHYSICS_HZ),
        total_ticks.saturating_sub(10 * PHYSICS_HZ),
        total_ticks.saturating_sub(5 * PHYSICS_HZ),
        total_ticks.saturating_sub(PHYSICS_HZ),
        total_ticks,
    ];
    let mut ticks = candidates
        .into_iter()
        .filter(|tick| *tick <= total_ticks)
        .collect::<Vec<_>>();
    ticks.sort_unstable();
    ticks.dedup();
    ticks
}

fn parse_rotations(value: Option<String>) -> Vec<ScheduledRotation> {
    let value = value.unwrap_or_else(|| usage());
    let mut rotations = value
        .split(',')
        .map(|entry| {
            let (tick, degrees) = entry.split_once(':').unwrap_or_else(|| usage());
            ScheduledRotation {
                tick: tick.parse::<u32>().unwrap_or_else(|_| usage()),
                degrees: degrees.parse::<i32>().unwrap_or_else(|_| usage()),
            }
        })
        .collect::<Vec<_>>();
    rotations.sort_unstable_by_key(|rotation| rotation.tick);
    if rotations
        .windows(2)
        .any(|pair| pair[0].tick == pair[1].tick)
    {
        usage();
    }
    rotations
}

fn parse_config() -> Config {
    let mut out = PathBuf::from("logs/hourglass_preview");
    let mut seconds = DEFAULT_DURATION_S;
    let mut scale = DEFAULT_SCALE;
    let mut gif_fps = DEFAULT_GIF_FPS;
    let mut gif_step_ticks = DEFAULT_GIF_STEP_TICKS;
    let mut png_at = None;
    let mut rotations = Vec::new();
    let mut arguments = env::args().skip(1);

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--out" => out = PathBuf::from(arguments.next().unwrap_or_else(|| usage())),
            "--seconds" => seconds = parse_positive(arguments.next()),
            "--scale" => scale = parse_positive(arguments.next()),
            "--gif-fps" => gif_fps = parse_positive(arguments.next()),
            "--gif-step-seconds" => {
                gif_step_ticks = parse_positive(arguments.next())
                    .checked_mul(PHYSICS_HZ)
                    .unwrap_or_else(|| usage());
            }
            "--gif-step-ticks" => gif_step_ticks = parse_positive(arguments.next()),
            "--png-at" => png_at = Some(arguments.next().unwrap_or_else(|| usage())),
            "--rotate" => rotations = parse_rotations(arguments.next()),
            "-h" | "--help" => usage(),
            _ => usage(),
        }
    }

    let total_ticks = seconds.checked_mul(PHYSICS_HZ).unwrap_or_else(|| usage());
    if rotations.iter().any(|rotation| rotation.tick > total_ticks) {
        usage();
    }
    let png_ticks = png_at
        .map(|value| parse_png_ticks(Some(value), total_ticks))
        .unwrap_or_else(|| default_png_ticks(total_ticks));
    Config {
        out,
        seconds,
        scale,
        gif_fps,
        gif_step_ticks,
        png_ticks,
        rotations,
    }
}

fn render_cells(model: &HourglassModel, scale: u32, device_tick: u32) -> GrayImage {
    let mut native = ImageBuffer::from_pixel(WIDTH, OUTPUT_HEIGHT, Luma([0xFF]));
    let projection = CellProjection::new(model.angle(), WIDTH as i32 / 2, HEIGHT as i32 / 2);
    for position in model.positions() {
        for (x, y) in projection.cell_pixels(position) {
            if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) {
                native.put_pixel(x as u32, y as u32, Luma([0x00]));
            }
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
    let label = format_device_time(device_tick);
    let mut x = 2;
    for character in label.bytes() {
        draw_glyph(image, x, HEIGHT + 2, glyph(character));
        x += 4;
    }
}

fn format_device_time(device_tick: u32) -> String {
    let total_seconds = device_tick / PHYSICS_HZ;
    format!("t={:02}:{:02}", total_seconds / 60, total_seconds % 60)
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

fn save_png(out: &Path, tick: u32, image: &GrayImage) -> image::ImageResult<()> {
    let name = if tick.is_multiple_of(PHYSICS_HZ) {
        format!("cells_{:04}s.png", tick / PHYSICS_HZ)
    } else {
        format!("cells_tick_{tick:06}.png")
    };
    image.save(out.join(name))
}

fn run(config: Config) -> Result<(), String> {
    fs::create_dir_all(&config.out)
        .map_err(|error| format!("create {}: {error}", config.out.display()))?;
    let gif_path = config.out.join("cells.gif");
    let gif_file = File::create(&gif_path)
        .map_err(|error| format!("create {}: {error}", gif_path.display()))?;
    let mut gif = GifEncoder::new_with_speed(BufWriter::new(gif_file), 10);
    gif.set_repeat(Repeat::Infinite)
        .map_err(|error| format!("configure {}: {error}", gif_path.display()))?;

    let mut model = HourglassModel::new();
    if !model.start() {
        return Err("fresh Hourglass model did not start".to_owned());
    }
    let total_ticks = config
        .seconds
        .checked_mul(PHYSICS_HZ)
        .ok_or_else(|| "preview duration overflows tick count".to_owned())?;
    let gif_step_ticks = config.gif_step_ticks;
    let mut next_png = 0;
    let mut next_rotation = 0;

    for tick in 0..=total_ticks {
        while next_rotation < config.rotations.len() && config.rotations[next_rotation].tick == tick
        {
            let rotation = config.rotations[next_rotation];
            let at_us = u64::from(tick)
                .saturating_mul(1_000_000)
                .checked_div(u64::from(PHYSICS_HZ))
                .unwrap_or(0);
            model
                .apply_command(RotationCommand {
                    kind: RotationCommandKind::RotateBy(Fx::ratio(rotation.degrees, 360)),
                    at: TimestampUs(at_us),
                })
                .map_err(|error| format!("rotation at tick {tick} was rejected: {error:?}"))?;
            next_rotation += 1;
        }
        let gif_frame = tick <= 1 || tick % gif_step_ticks == 0 || tick == total_ticks;
        let png_frame = next_png < config.png_ticks.len() && tick == config.png_ticks[next_png];
        if gif_frame || png_frame {
            let image = render_cells(&model, config.scale, tick);
            if gif_frame {
                encode_gif_frame(&mut gif, image.clone(), config.gif_fps)
                    .map_err(|error| format!("encode {}: {error}", gif_path.display()))?;
            }
            if png_frame {
                save_png(&config.out, tick, &image).map_err(|error| {
                    format!(
                        "save PNG at tick {tick} in {}: {error}",
                        config.out.display()
                    )
                })?;
                next_png += 1;
            }
        }
        if tick < total_ticks {
            model.tick();
        }
    }

    println!(
        "hourglass preview complete: simulated={}s grains={} backend=paper-cellular gravity_schedule={} rotations={} gif={} pngs={} output={} resolution={}x{}",
        config.seconds,
        PARTICLE_CAPACITY,
        GRAVITY_SCHEDULE,
        config.rotations.len(),
        gif_path.display(),
        config.png_ticks.len(),
        config.out.display(),
        WIDTH * config.scale,
        OUTPUT_HEIGHT * config.scale,
    );
    Ok(())
}

fn main() {
    if let Err(error) = run(parse_config()) {
        eprintln!("hourglass preview failed: {error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_model_renders_particle_cells_and_time_below_them() {
        let model = HourglassModel::new();
        let image = render_cells(&model, 1, 0);
        assert_eq!(image.dimensions(), (WIDTH, OUTPUT_HEIGHT));
        let cell_ink = (0..HEIGHT)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .filter(|(x, y)| image.get_pixel(*x, *y).0[0] == 0)
            .count();
        let label_ink = (HEIGHT..OUTPUT_HEIGHT)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .filter(|(x, y)| image.get_pixel(*x, *y).0[0] == 0)
            .count();
        assert!(cell_ink <= model.positions().len());
        assert!(cell_ink >= model.positions().len() * 95 / 100);
        assert!(label_ink > 0);
    }

    #[test]
    fn time_label_uses_simulated_device_ticks() {
        let model = HourglassModel::new();
        let start = render_cells(&model, 1, 0);
        let later = render_cells(&model, 1, PHYSICS_HZ * 61);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                assert_eq!(start.get_pixel(x, y), later.get_pixel(x, y));
            }
        }
        assert!((HEIGHT..OUTPUT_HEIGHT)
            .any(|y| { (0..WIDTH).any(|x| start.get_pixel(x, y) != later.get_pixel(x, y)) }));
    }

    #[test]
    fn device_time_is_elapsed_model_time() {
        assert_eq!(format_device_time(0), "t=00:00");
        assert_eq!(format_device_time(PHYSICS_HZ * 59), "t=00:59");
        assert_eq!(format_device_time(PHYSICS_HZ * 60), "t=01:00");
        assert_eq!(format_device_time(PHYSICS_HZ * 900), "t=15:00");
    }

    #[test]
    fn default_snapshot_schedule_is_dense_near_completion() {
        let ticks = default_png_ticks(900 * PHYSICS_HZ);
        assert_eq!(ticks.first(), Some(&0));
        assert_eq!(ticks.last(), Some(&(900 * PHYSICS_HZ)));
        assert!(ticks.contains(&1));
        assert!(ticks.contains(&(30 * PHYSICS_HZ)));
        assert!(ticks.contains(&(100 * PHYSICS_HZ)));
        assert!(ticks.contains(&(899 * PHYSICS_HZ)));
        assert!(ticks.contains(&(895 * PHYSICS_HZ)));
    }

    #[test]
    fn rotation_trace_is_sorted_and_keeps_signed_angles() {
        assert_eq!(
            parse_rotations(Some("900:-30,1:15,1800:105".to_owned())),
            vec![
                ScheduledRotation {
                    tick: 1,
                    degrees: 15,
                },
                ScheduledRotation {
                    tick: 900,
                    degrees: -30,
                },
                ScheduledRotation {
                    tick: 1800,
                    degrees: 105,
                },
            ]
        );
    }
}
