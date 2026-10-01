//! Host preview for the ensō stroke, matching `.scratch/extract/geoink_out.py`'s
//! three modes.
//!
//! Renders through the same `no_std` model the firmware would use, at the
//! panel's own one bit per pixel, with no host-only smoothing anywhere in the
//! path -- every cell and frame is rendered at the size it is shown at, never
//! scaled up or down, so a PNG is exactly what the panel would be sent.
//!
//! ```sh
//! cargo run -p enso --release --example enso_preview -- sheet 900 12 200
//! cargo run -p enso --release --example enso_preview -- growth 906
//! cargo run -p enso --release --example enso_preview -- gif 906 60 45
//! ```

use std::env;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::PathBuf;
use std::process;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, GrayImage, Luma, RgbaImage};

use enso::Stroke;
use raster::{BitCanvas, Dither};

/// Panel edge for `growth` and `gif`, in pixels. This is the ensō's real
/// target resolution, not a preview size -- see the module doc.
const NATIVE_SIZE: i32 = 600;

/// Milliseconds each `gif` frame holds, and how long the finished stroke
/// holds before the loop restarts. Matches `geoink_out.py`'s own
/// `duration=[42]*(n-1)+[1800]`.
const GIF_FRAME_MS: u32 = 42;
const GIF_HOLD_MS: u32 = 1800;

const GROWTH_POINTS: [u32; 5] = [20, 40, 60, 80, 100];

fn usage() -> ! {
    eprintln!(
        "usage: cargo run -p enso --release --example enso_preview -- <mode> [args]\n\
         \n\
         Modes:\n\
           sheet BASE N CELL        Contact sheet of N seeds from BASE, each\n\
                                    rendered natively at CELL px, numbered.\n\
           growth SEED              One seed's stroke at 20/40/60/80/100%\n\
                                    progress, as a strip, each panel\n\
                                    {NATIVE_SIZE}px.\n\
           gif SEED FRAMES SESSION_MINUTES\n\
                                    Animated growth from 0 to 100% progress at\n\
                                    {NATIVE_SIZE}px, with an elapsed-session\n\
                                    footer. SESSION_MINUTES is what the footer's\n\
                                    clock counts against, independent of how\n\
                                    many FRAMES the animation actually has.\n\
         \n\
         Common options (after the mode's own arguments):\n\
           --out DIR      Output directory (default: .scratch/rustproto)\n\
           --dither MODE  gradient | bayer4 | none (default: gradient)\n\
         \n\
         Output is one bit per pixel at native size, dithered but never scaled\n\
         or smoothed -- what a PNG shows is what the panel would be sent."
    );
    process::exit(2);
}

enum Mode {
    Sheet {
        base: u32,
        n: u32,
        cell: i32,
    },
    Growth {
        seed: u32,
    },
    Gif {
        seed: u32,
        frames: u32,
        session_minutes: u32,
    },
}

struct Config {
    mode: Mode,
    out: PathBuf,
    dither: Dither,
}

fn parse_u32(value: Option<&str>) -> u32 {
    value
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| usage())
}

fn parse_positive_i32(value: Option<&str>) -> i32 {
    value
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or_else(|| usage())
}

fn parse_config() -> Config {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut it = args.iter();
    let mode_name = it.next().map(String::as_str).unwrap_or_else(|| usage());

    let mode = match mode_name {
        "sheet" => {
            let base = parse_u32(it.next().map(String::as_str));
            let n = parse_u32(it.next().map(String::as_str)).max(1);
            let cell = parse_positive_i32(it.next().map(String::as_str));
            Mode::Sheet { base, n, cell }
        }
        "growth" => {
            let seed = parse_u32(it.next().map(String::as_str));
            Mode::Growth { seed }
        }
        "gif" => {
            let seed = parse_u32(it.next().map(String::as_str));
            let frames = parse_u32(it.next().map(String::as_str)).max(2);
            let session_minutes = parse_u32(it.next().map(String::as_str)).max(1);
            Mode::Gif {
                seed,
                frames,
                session_minutes,
            }
        }
        "-h" | "--help" => usage(),
        _ => usage(),
    };

    let mut out = PathBuf::from(".scratch/rustproto");
    let mut dither = Dither::Gradient;
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--out" => out = PathBuf::from(it.next().unwrap_or_else(|| usage())),
            "--dither" => {
                dither = match it.next().map(String::as_str).unwrap_or_else(|| usage()) {
                    "gradient" | "ign" => Dither::Gradient,
                    "bayer4" | "bayer" => Dither::Bayer4,
                    "none" => Dither::None,
                    _ => usage(),
                }
            }
            _ => usage(),
        }
    }

    Config { mode, out, dither }
}

/// A one-bit canvas plus the buffer backing it, so callers can hand the pair
/// around without fighting the borrow that `BitCanvas` holds.
struct Panel {
    size: i32,
    bits: Vec<u8>,
}

impl Panel {
    fn new(size: i32) -> Self {
        Self {
            size,
            bits: vec![0u8; BitCanvas::bytes_for(size, size)],
        }
    }

    fn canvas(&mut self) -> BitCanvas<'_> {
        BitCanvas::new(self.size, self.size, &mut self.bits).expect("canvas dimensions")
    }

    fn snapshot(&mut self) -> GrayImage {
        let size = self.size;
        let canvas = self.canvas();
        GrayImage::from_fn(size as u32, size as u32, |x, y| {
            Luma([if canvas.get(x as i32, y as i32) {
                0
            } else {
                255
            }])
        })
    }
}

fn render_panel(seed: u32, size: i32, progress: f32, dither: Dither) -> GrayImage {
    let stroke = Stroke::from_seed(seed, size as f32);
    let mut panel = Panel::new(size);
    {
        let mut canvas = panel.canvas();
        stroke.render(progress, dither, &mut canvas);
    }
    panel.snapshot()
}

fn print_summary(seed: u32, size: f32) {
    let stroke = Stroke::from_seed(seed, size);
    let p = stroke.params();
    let wobble: f32 = p.wobble.iter().map(|h| h.amplitude).sum();
    println!(
        "seed {seed} -- entry {:?}, exit {:?}, {} \n  \
         radius {:.0}px, width {:.0}px, sweep {:.0}deg, wobble {:.2}%\n  \
         weight_start {:.2}, ink_start {:.2}, bleed {:.2}, roll {:+.2}, plant {:.2}, flick {:+.2}",
        p.entry,
        p.exit,
        if p.direction > 0.0 {
            "clockwise"
        } else {
            "anticlockwise"
        },
        p.radius,
        p.width,
        p.sweep.to_degrees(),
        wobble * 100.0,
        p.weight_start,
        p.ink_start,
        p.bleed,
        p.roll,
        p.plant,
        p.flick,
    );
}

/// Numbered contact sheet: `n` seeds from `base`, each rendered natively at
/// `cell` px, its seed printed in the cell's top-left corner. The ensō is
/// inset from its cell, so the corner is always clear.
fn render_sheet(base: u32, n: u32, cell: i32, out: &PathBuf, dither: Dither) {
    let columns = (n as f32).sqrt().ceil() as u32;
    let rows = n.div_ceil(columns);
    let gap = 3u32;
    let width = columns * cell as u32 + (columns + 1) * gap;
    let height = rows * cell as u32 + (rows + 1) * gap;
    let mut sheet = GrayImage::from_pixel(width, height, Luma([205]));

    for index in 0..n {
        let seed = base.wrapping_add(index);
        let gray = render_panel(seed, cell, 1.0, dither);
        let column = index % columns;
        let row = index / columns;
        let x0 = gap + column * (cell as u32 + gap);
        let y0 = gap + row * (cell as u32 + gap);
        for y in 0..cell as u32 {
            for x in 0..cell as u32 {
                sheet.put_pixel(x0 + x, y0 + y, *gray.get_pixel(x, y));
            }
        }
        let label_scale = (cell as u32 / 32).max(2);
        let mut x = x0 + 2 * label_scale;
        for character in format!("{seed}").bytes() {
            draw_glyph(
                &mut sheet,
                x,
                y0 + 2 * label_scale,
                glyph(character),
                label_scale,
            );
            x += 4 * label_scale;
        }
    }

    fs::create_dir_all(out).expect("create output directory");
    let path = out.join(format!("sheet-{base}-{n}.png"));
    sheet.save(&path).expect("write contact sheet");
    println!("{n} seeds from {base} in a {columns}x{rows} grid of {cell}px cells");
    println!("  {}", path.display());
}

/// One seed's stroke as a strip at five progress points, each panel labelled
/// with its percentage.
fn render_growth(seed: u32, out: &PathBuf, dither: Dither) {
    print_summary(seed, NATIVE_SIZE as f32);

    let gap = 4u32;
    let cell = NATIVE_SIZE as u32;
    let count = GROWTH_POINTS.len() as u32;
    let width = count * cell + (count + 1) * gap;
    let mut strip = GrayImage::from_pixel(width, cell, Luma([205]));

    for (i, percent) in GROWTH_POINTS.iter().enumerate() {
        let gray = render_panel(seed, NATIVE_SIZE, *percent as f32 / 100.0, dither);
        let x0 = gap + i as u32 * (cell + gap);
        for y in 0..cell {
            for x in 0..cell {
                strip.put_pixel(x0 + x, y, *gray.get_pixel(x, y));
            }
        }
        let scale = 3u32;
        let mut x = x0 + 2 * scale;
        for character in format!("{percent}%").bytes() {
            draw_glyph(&mut strip, x, cell - 8 * scale, glyph(character), scale);
            x += 4 * scale;
        }
    }

    fs::create_dir_all(out).expect("create output directory");
    let path = out.join(format!("growth-{seed}.png"));
    strip.save(&path).expect("write growth strip");
    println!("  {}", path.display());
}

/// Animated growth from zero to full progress, with an elapsed-session
/// footer below the image -- the image itself carries no footer, so it stays
/// byte-faithful to what a single frame on the panel would be.
fn render_gif(seed: u32, frames: u32, session_minutes: u32, out: &PathBuf, dither: Dither) {
    print_summary(seed, NATIVE_SIZE as f32);
    let stroke = Stroke::from_seed(seed, NATIVE_SIZE as f32);

    fs::create_dir_all(out).expect("create output directory");
    let path = out.join(format!("enso-{seed:08}.gif"));
    let file = File::create(&path).expect("create gif");
    let mut encoder = GifEncoder::new(BufWriter::new(file));
    encoder.set_repeat(Repeat::Infinite).expect("gif repeat");

    // Each frame is a complete redraw from a cleared canvas, which is what
    // the device would do too. The per-frame diff is tracked because it is
    // the real cost of this design: pixels that go black-to-white are reverse
    // transitions, and those are what accumulate ghosting on the panel -- see
    // `docs/references/display-refresh.md`.
    let mut panel = Panel::new(NATIVE_SIZE);
    let mut previous_bits: Option<Vec<u8>> = None;
    let mut worst_changed = 0u32;
    let mut worst_cleared = 0u32;
    let mut total_changed = 0u64;
    let mut total_cleared = 0u64;
    let total_seconds = session_minutes * 60;

    for frame_index in 0..=frames {
        let progress = frame_index as f32 / frames as f32;
        panel.bits.fill(0);
        {
            let mut canvas = panel.canvas();
            stroke.render(progress, dither, &mut canvas);
        }

        if let Some(earlier) = &previous_bits {
            let mut inked = 0u32;
            let mut cleared = 0u32;
            for (before, after) in earlier.iter().zip(panel.bits.iter()) {
                inked += (!before & after).count_ones();
                cleared += (before & !after).count_ones();
            }
            worst_changed = worst_changed.max(inked + cleared);
            worst_cleared = worst_cleared.max(cleared);
            total_changed += u64::from(inked + cleared);
            total_cleared += u64::from(cleared);
        }
        previous_bits = Some(panel.bits.clone());

        let gray = panel.snapshot();
        let elapsed = (progress * total_seconds as f32) as u32;
        let labelled = with_time_footer(&gray, elapsed, total_seconds);
        let rgba = RgbaImage::from_fn(labelled.width(), labelled.height(), |x, y| {
            let level = labelled.get_pixel(x, y).0[0];
            image::Rgba([level, level, level, 255])
        });

        let delay = if frame_index == frames {
            Delay::from_numer_denom_ms(GIF_HOLD_MS, 1)
        } else {
            Delay::from_numer_denom_ms(GIF_FRAME_MS, 1)
        };
        encoder
            .encode_frame(Frame::from_parts(rgba, 0, 0, delay))
            .expect("encode frame");
    }
    drop(encoder);
    println!("  {}", path.display());
    if frames > 0 {
        println!(
            "  frame deltas over {frames} steps: mean {} px changed ({} of them \
             black->white), worst {worst_changed} ({worst_cleared})",
            total_changed / u64::from(frames),
            total_cleared / u64::from(frames)
        );
    }
}

/// Copies `image` onto a taller canvas and writes the elapsed session time
/// underneath it.
fn with_time_footer(image: &GrayImage, elapsed: u32, total: u32) -> GrayImage {
    let scale = (image.width() / 150).max(1);
    let footer = 5 * scale + 4 * scale;
    let mut out = GrayImage::from_pixel(image.width(), image.height() + footer, Luma([255]));
    for y in 0..image.height() {
        for x in 0..image.width() {
            out.put_pixel(x, y, *image.get_pixel(x, y));
        }
    }

    let label = format!(
        "t={:02}:{:02} / {:02}:{:02}",
        elapsed / 60,
        elapsed % 60,
        total / 60,
        total % 60
    );
    let mut x = 2 * scale;
    for character in label.bytes() {
        draw_glyph(
            &mut out,
            x,
            image.height() + 2 * scale,
            glyph(character),
            scale,
        );
        x += 4 * scale;
    }
    out
}

fn draw_glyph(image: &mut GrayImage, x: u32, y: u32, rows: [u8; 5], scale: u32) {
    for (row, bits) in rows.into_iter().enumerate() {
        for column in 0..3u32 {
            if bits & (1 << (2 - column)) == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let px = x + column * scale + dx;
                    let py = y + row as u32 * scale + dy;
                    if px < image.width() && py < image.height() {
                        image.put_pixel(px, py, Luma([0]));
                    }
                }
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
        b'/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        b'%' => [0b101, 0b001, 0b010, 0b100, 0b101],
        _ => [0; 5],
    }
}

fn main() {
    let config = parse_config();
    match config.mode {
        Mode::Sheet { base, n, cell } => render_sheet(base, n, cell, &config.out, config.dither),
        Mode::Growth { seed } => render_growth(seed, &config.out, config.dither),
        Mode::Gif {
            seed,
            frames,
            session_minutes,
        } => render_gif(seed, frames, session_minutes, &config.out, config.dither),
    }
}
