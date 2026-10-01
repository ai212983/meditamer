//! Rasterizes digits into packed one-bit "cards" for the split-flap renderer,
//! emitted as Rust source for a build script to write into `OUT_DIR`.
//!
//! Separate from `tools/lvgl_font_compiler` because the output is a different
//! artifact for a different consumer: that one emits `lv_font_fmt_txt` tables
//! for LVGL to lay out and draw, this one emits plain row-major bitmaps that
//! `apps::flipclock` resamples itself. A flap has to be scaled and sheared per
//! frame, which is not something an LVGL font can be asked to do.
//!
//! The two do share their threshold. Coverage at or above 128 becomes a set
//! bit, matching the panel flush path, so a glyph reaches the glass exactly as
//! the rasterizer drew it. There is no anti-aliasing to preserve: the panel is
//! one bit and the flush path thresholds at mid-grey, so a 4bpp ramp would
//! quadruple the table without surviving.

use std::{fmt::Write as _, fs, path::Path};

use fontdue::{Font, FontSettings};

/// Coverage at or above this becomes a set bit. Matches
/// `lvgl_font_compiler::COVERAGE_THRESHOLD` and the flush path's threshold.
const COVERAGE_THRESHOLD: u8 = 128;

/// Bytes of bitmap per emitted source line.
const BYTES_PER_LINE: usize = 15;

pub struct CardSpec<'a> {
    /// TrueType face to rasterize. A variable face renders at its default
    /// instance.
    pub source: &'a Path,
    /// Card size in pixels. Width is rounded up to a byte boundary in the
    /// packed rows; height is exact.
    pub card_w: i32,
    pub card_h: i32,
    /// Rendering size in pixels per em.
    pub size_px: f32,
    /// Characters to emit, in order. Index into `DIGIT_CARDS` is the position
    /// here, not the character's value.
    pub characters: &'a str,
}

#[derive(Debug)]
pub enum CardCompilerError {
    Read(String, std::io::Error),
    Parse(String),
    /// A glyph did not fit the card at the requested size. Silently clipping a
    /// digit would be worse than failing the build.
    Overflow {
        character: char,
        width: usize,
        height: usize,
    },
}

impl std::fmt::Display for CardCompilerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(path, error) => write!(f, "reading {path}: {error}"),
            Self::Parse(message) => write!(f, "parsing face: {message}"),
            Self::Overflow {
                character,
                width,
                height,
            } => write!(
                f,
                "glyph '{character}' rasterized to {width}x{height}, which does not fit the card"
            ),
        }
    }
}

impl std::error::Error for CardCompilerError {}

struct Rasterized {
    character: char,
    width: usize,
    height: usize,
    /// Distance from the baseline to the bitmap's bottom row; negative when
    /// the glyph descends below it.
    ymin: i32,
    coverage: Vec<u8>,
}

/// Emits `CARD_W`, `CARD_H`, `CARD_BYTES` and `DIGIT_CARDS` as Rust source.
///
/// The geometry constants are emitted rather than duplicated in the consuming
/// crate: a build script cannot import the crate it is building, so the
/// generated file is the single source of truth and `apps::flipclock` reads
/// its card size from there.
pub fn generate(spec: &CardSpec<'_>) -> Result<String, CardCompilerError> {
    let bytes = fs::read(spec.source)
        .map_err(|error| CardCompilerError::Read(spec.source.display().to_string(), error))?;
    let font = Font::from_bytes(bytes, FontSettings::default())
        .map_err(|error| CardCompilerError::Parse(error.to_string()))?;

    let mut glyphs = Vec::new();
    for character in spec.characters.chars() {
        let (metrics, coverage) = font.rasterize(character, spec.size_px);
        if metrics.width > spec.card_w as usize || metrics.height > spec.card_h as usize {
            return Err(CardCompilerError::Overflow {
                character,
                width: metrics.width,
                height: metrics.height,
            });
        }
        glyphs.push(Rasterized {
            character,
            width: metrics.width,
            height: metrics.height,
            ymin: metrics.ymin,
            coverage,
        });
    }

    // One shared baseline for every digit, rather than centring each on its own
    // ink box. Round figures (0, 3, 6, 8) overshoot the cap line slightly, so
    // per-glyph centring would make them sit a pixel high and the row would
    // visibly jitter as the clock ticks.
    let highest = glyphs
        .iter()
        .map(|glyph| glyph.height as i32 + glyph.ymin)
        .max()
        .unwrap_or(0);
    let lowest = glyphs.iter().map(|glyph| glyph.ymin).min().unwrap_or(0);
    let baseline = (spec.card_h - (highest - lowest)) / 2 + highest;

    let stride = ((spec.card_w + 7) / 8) as usize;
    let card_bytes = stride * spec.card_h as usize;

    let mut cards = Vec::new();
    for glyph in &glyphs {
        let mut card = vec![0u8; card_bytes];
        // Horizontal centring stays per glyph: the face is proportional, so a
        // '1' really is narrower and should sit in the middle of its own card.
        let origin_x = (spec.card_w - glyph.width as i32) / 2;
        let origin_y = baseline - (glyph.height as i32 + glyph.ymin);
        for row in 0..glyph.height as i32 {
            for column in 0..glyph.width as i32 {
                let index = (row * glyph.width as i32 + column) as usize;
                if glyph.coverage[index] < COVERAGE_THRESHOLD {
                    continue;
                }
                let x = origin_x + column;
                let y = origin_y + row;
                if x < 0 || y < 0 || x >= spec.card_w || y >= spec.card_h {
                    continue;
                }
                card[y as usize * stride + (x >> 3) as usize] |= 0x80 >> (x & 7);
            }
        }
        cards.push(card);
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "// @generated by tools/flip_digit_compiler from {} at {} px, 1 bpp.\n\
         // Do not edit; change the build script or the spec instead.",
        spec.source.display(),
        spec.size_px,
    );
    let _ = writeln!(out, "pub const CARD_W: i32 = {};", spec.card_w);
    let _ = writeln!(out, "pub const CARD_H: i32 = {};", spec.card_h);
    let _ = writeln!(out, "pub const CARD_STRIDE: usize = {stride};");
    let _ = writeln!(out, "pub const CARD_BYTES: usize = {card_bytes};");
    let _ = writeln!(
        out,
        "pub static DIGIT_CARDS: [[u8; CARD_BYTES]; {}] = [",
        cards.len()
    );
    for (card, glyph) in cards.iter().zip(&glyphs) {
        let _ = writeln!(out, "    // '{}'", glyph.character);
        let _ = writeln!(out, "    [");
        for chunk in card.chunks(BYTES_PER_LINE) {
            let _ = write!(out, "       ");
            for byte in chunk {
                let _ = write!(out, " 0x{byte:02X},");
            }
            let _ = writeln!(out);
        }
        let _ = writeln!(out, "    ],");
    }
    let _ = writeln!(out, "];");
    Ok(out)
}
