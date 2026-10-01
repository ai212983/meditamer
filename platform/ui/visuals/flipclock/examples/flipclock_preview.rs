//! Host preview for the split-flap digit renderer.
//!
//! Renders the twelve-frame flip through the same `no_std` rasterizer the
//! firmware would use, so what the contact sheet shows is what the panel would
//! get after its threshold -- there is no host-only smoothing anywhere in the
//! path. Digits come from the repository's own IBM Plex face at the same 128
//! coverage threshold `tools/lvgl_font_compiler` uses.
//!
//! ```sh
//! cargo run -p flipclock --release --example flipclock_preview -- --out .scratch/flip
//! ```

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

use flipclock::digits::{CARD_H, CARD_W};
use flipclock::flap::{self, Card};
use flipclock::housing;
use flipclock::screen::Screen;
use flipclock::{FlipClock, FLIP_DURATION_MS, PREVIEW_FRAMES};
use flipclock::{CAMERA_D, HINGE_X, HINGE_Y, SURFACE_H, SURFACE_W};
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, GrayImage, Luma, RgbaImage};
use raster::Surface;

struct Canvas {
    width: i32,
    height: i32,
    pixels: Vec<bool>,
}

impl Canvas {
    fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            pixels: vec![false; (width * height) as usize],
        }
    }

    fn to_gray(&self) -> GrayImage {
        GrayImage::from_fn(self.width as u32, self.height as u32, |x, y| {
            let ink = self.pixels[(y as i32 * self.width + x as i32) as usize];
            Luma([if ink { 0 } else { 255 }])
        })
    }
}

impl Surface for Canvas {
    fn width(&self) -> i32 {
        self.width
    }
    fn height(&self) -> i32 {
        self.height
    }
    fn set(&mut self, x: i32, y: i32, ink: bool) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        self.pixels[(y * self.width + x) as usize] = ink;
    }
}

/// Samples the flip at `frame` of [`PREVIEW_FRAMES`], through the same clock
/// the firmware drives -- so the preview cannot animate differently from the
/// device, only at different instants.
fn render_frame_image(card: &Card, digit: u8, frame: u32, screen: Screen) -> Canvas {
    let elapsed = frame * FLIP_DURATION_MS / (PREVIEW_FRAMES - 1);
    let clock = if frame == 0 {
        FlipClock::new(digit)
    } else {
        FlipClock::at(digit, elapsed)
    };
    let mut canvas = Canvas::new(SURFACE_W, SURFACE_H);
    housing::draw_ground(&mut canvas, card, screen);
    flap::render_frame(&mut canvas, card, clock.pose(), &clock.faces(), screen);
    housing::draw_rails(&mut canvas, card, screen);
    canvas
}

fn scale(image: &GrayImage, factor: u32) -> GrayImage {
    GrayImage::from_fn(image.width() * factor, image.height() * factor, |x, y| {
        *image.get_pixel(x / factor, y / factor)
    })
}

/// Contact sheet: every frame at `factor`, in one row per phase, so the two
/// halves of the flip can be compared against each other directly.
fn contact_sheet(frames: &[GrayImage], factor: u32) -> GrayImage {
    let columns = PREVIEW_FRAMES / 2;
    let rows = (frames.len() as u32).div_ceil(columns);
    let cell_w = frames[0].width() * factor + 8;
    let cell_h = frames[0].height() * factor + 8;
    let mut sheet = GrayImage::from_pixel(cell_w * columns, cell_h * rows, Luma([160]));
    for (index, frame) in frames.iter().enumerate() {
        let scaled = scale(frame, factor);
        let ox = (index as u32 % columns) * cell_w + 4;
        let oy = (index as u32 / columns) * cell_h + 4;
        for y in 0..scaled.height() {
            for x in 0..scaled.width() {
                sheet.put_pixel(ox + x, oy + y, *scaled.get_pixel(x, y));
            }
        }
    }
    sheet
}

/// Tone ramps at 1:1 through every screen, stacked for comparison. This is the
/// sheet to photograph on the panel: the question is not which looks best here
/// but which still reads as grey rather than as texture on reflective glass.
fn screen_ramps() -> GrayImage {
    let screens = [Screen::Bayer, Screen::BlueNoise, Screen::Clustered];
    let levels: [u8; 7] = [0, 2, 4, 6, 8, 12, 16];
    let band = 44u32;
    let row_h = 44u32;
    let gap = 6u32;
    GrayImage::from_fn(
        band * levels.len() as u32,
        (row_h + gap) * screens.len() as u32,
        |x, y| {
            let row = (y / (row_h + gap)) as usize;
            let local_y = y % (row_h + gap);
            if local_y >= row_h {
                return Luma([200]);
            }
            let level = levels[(x / band) as usize];
            Luma([if screens[row].ink(x as i32, local_y as i32, level) {
                0
            } else {
                255
            }])
        },
    )
}

fn main() {
    let mut out = PathBuf::from(".scratch/flip");
    let mut camera_d = CAMERA_D;
    let mut digit = 2u8;
    let mut screen = Screen::BlueNoise;

    let args: Vec<String> = env::args().skip(1).collect();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].clone();
        let next = || {
            args.get(index + 1).cloned().unwrap_or_else(|| {
                eprintln!("missing value for {flag}");
                process::exit(2);
            })
        };
        match args[index].as_str() {
            "--out" => out = PathBuf::from(next()),
            "--camera-d" => camera_d = next().parse().expect("camera-d"),
            "--digit" => digit = next().parse().expect("digit"),
            "--screen" => {
                screen = match next().as_str() {
                    "bayer" => Screen::Bayer,
                    "blue" => Screen::BlueNoise,
                    "clustered" => Screen::Clustered,
                    other => {
                        eprintln!("unknown screen {other}");
                        process::exit(2);
                    }
                }
            }
            other => {
                eprintln!("unknown option {other}");
                process::exit(2);
            }
        }
        index += 2;
    }

    fs::create_dir_all(&out).expect("create output directory");
    let card = Card {
        hinge_x: HINGE_X,
        hinge_y: HINGE_Y,
        half_w: CARD_W / 2,
        half_h: CARD_H / 2,
        camera_d,
    };

    let mut frames = Vec::new();
    for index in 0..PREVIEW_FRAMES {
        let canvas = render_frame_image(&card, digit, index, screen);
        let image = canvas.to_gray();
        image
            .save(out.join(format!("frame_{index:02}.png")))
            .expect("write frame");
        frames.push(image);
    }

    contact_sheet(&frames, 2)
        .save(out.join("sheet.png"))
        .expect("write sheet");
    // 1:1 as well: scaling up exaggerates ordered dither badly, and the
    // question is what it looks like at the panel's own pixel pitch.
    contact_sheet(&frames, 1)
        .save(out.join("sheet_1x.png"))
        .expect("write 1x sheet");
    // The single high-tilt still is the one that decides whether the taper,
    // edge bar and cast shadow read as three-dimensional at all.
    scale(&frames[3], 4)
        .save(out.join("still_tilted.png"))
        .expect("write still");
    screen_ramps()
        .save(out.join("screen_ramps.png"))
        .expect("write ramps");

    // The same tilted still under every screen, side by side at 4x. The three
    // differ most exactly where this design spends its tone: a large flat
    // region of light grey.
    let comparison: Vec<GrayImage> = [Screen::Bayer, Screen::BlueNoise, Screen::Clustered]
        .into_iter()
        .map(|s| render_frame_image(&card, digit, 3, s).to_gray())
        .collect();
    let cell_w = comparison[0].width() * 3 + 8;
    let mut strip = GrayImage::from_pixel(cell_w * 3, comparison[0].height() * 3 + 8, Luma([170]));
    for (slot, image) in comparison.iter().enumerate() {
        let scaled = scale(image, 3);
        for y in 0..scaled.height() {
            for x in 0..scaled.width() {
                strip.put_pixel(slot as u32 * cell_w + 4 + x, 4 + y, *scaled.get_pixel(x, y));
            }
        }
    }
    strip
        .save(out.join("screen_comparison.png"))
        .expect("write comparison");

    let gif_file = fs::File::create(out.join("flip.gif")).expect("create gif");
    let mut encoder = GifEncoder::new(std::io::BufWriter::new(gif_file));
    encoder.set_repeat(Repeat::Infinite).expect("gif repeat");
    for image in &frames {
        let scaled = scale(image, 3);
        let rgba = RgbaImage::from_fn(scaled.width(), scaled.height(), |x, y| {
            let Luma([value]) = *scaled.get_pixel(x, y);
            image::Rgba([value, value, value, 255])
        });
        encoder
            .encode_frame(Frame::from_parts(
                rgba,
                0,
                0,
                Delay::from_numer_denom_ms(38, 1),
            ))
            .expect("gif frame");
    }

    println!("wrote {} frames to {}", frames.len(), out.display());
}
