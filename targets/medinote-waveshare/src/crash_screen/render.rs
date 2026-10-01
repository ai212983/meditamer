use esp_hal::gpio::{Level, Output, OutputConfig, Pull};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::Rate;

use waveshare_rlcd42::panel;

use super::font::{glyph_for, Glyph, BITMAPS};

/// Each source pixel becomes a `SCALE`x`SCALE` block. Left at 1 --
/// deliberately no scaling in either direction. Two prior attempts here
/// (Picopixel at 3x up, FreeSans9pt7b at 2x up) both got worse, not better,
/// from block-scaling: it turns a bitmap font's own pixels into visible
/// chunks rather than making text read bigger. See the module doc's
/// "History" section before reaching for this again -- the fix for wanting
/// a different size was picking a font drawn at that size, not scaling
/// this one.
pub(super) const SCALE: i32 = 1;

pub(super) const MARGIN: i32 = 8;

/// Direct port of `Adafruit_GFX::drawChar`'s custom-font branch: unpack
/// `width * height` bits, MSB-first, packed contiguously (no row padding),
/// starting at `glyph.bitmap_offset`, and plot each set bit at
/// `(cursor_x + xOffset + xx, cursor_y + yOffset + yy)` in font units,
/// scaled by `SCALE`.
pub(super) fn draw_glyph(display: &mut panel::St7305, cursor_x: i32, cursor_y: i32, glyph: &Glyph) {
    let mut bit_index: u32 = 0;
    let mut byte: u8 = 0;
    let mut offset = glyph.bitmap_offset as usize;
    for yy in 0..glyph.height as i32 {
        for xx in 0..glyph.width as i32 {
            if bit_index & 7 == 0 {
                byte = BITMAPS.get(offset).copied().unwrap_or(0);
                offset += 1;
            }
            bit_index += 1;
            let set = byte & 0x80 != 0;
            byte <<= 1;
            if set {
                let px = cursor_x + (glyph.x_offset as i32 + xx) * SCALE;
                let py = cursor_y + (glyph.y_offset as i32 + yy) * SCALE;
                fill_block(display, px, py, SCALE, SCALE);
            }
        }
    }
}

pub(super) fn fill_block(display: &mut panel::St7305, x: i32, y: i32, w: i32, h: i32) {
    for dy in 0..h {
        for dx in 0..w {
            let (px, py) = (x + dx, y + dy);
            if px >= 0 && py >= 0 {
                display.set_pixel(px as usize, py as usize, true);
            }
        }
    }
}

/// Draws one line of text, left to right, stopping (not wrapping) once the
/// next glyph would cross the panel's right edge. For fixed, known-short
/// text only (the heading) -- anything coming from `PanicInfo` should use
/// [`draw_text_wrapped`] instead, or it silently loses everything past the
/// edge rather than wrapping. Characters outside Picopixel's `0x20..=0x7E`
/// range are skipped rather than drawn as a placeholder box, since a
/// missing glyph here is expected (panic messages are arbitrary Rust
/// `Display` output) rather than an error.
pub(super) fn draw_text(display: &mut panel::St7305, start_x: i32, baseline_y: i32, text: &str) {
    let mut cursor_x = start_x;
    for c in text.chars() {
        let Some(glyph) = glyph_for(c) else { continue };
        let advance = glyph.x_advance as i32 * SCALE;
        if cursor_x + advance > panel::WIDTH as i32 {
            break;
        }
        draw_glyph(display, cursor_x, baseline_y, glyph);
        cursor_x += advance;
    }
}

/// Like [`draw_text`], but wraps onto additional lines (`line_height` px
/// apart) instead of dropping everything past the first line's right edge --
/// panic messages are arbitrary length and this was truncating them
/// mid-word (`meditamer-guru-meditation-screen-probe.md`'s hardware check
/// caught this on the very first real message). Character-wrapping, not
/// word-wrapping: a v1 simplification that can break mid-word, traded for
/// not needing a second pass or a lookahead buffer to find word breaks.
///
/// Stops (not just wraps) once `cursor_y` would exceed `max_y`, so this
/// can't run into whatever the caller draws below it. Returns the y
/// coordinate immediately below the last line actually drawn, for the
/// caller to place the next thing.
pub(super) fn draw_text_wrapped(
    display: &mut panel::St7305,
    start_x: i32,
    start_y: i32,
    line_height: i32,
    max_y: i32,
    text: &str,
) -> i32 {
    let mut cursor_x = start_x;
    let mut cursor_y = start_y;
    for c in text.chars() {
        let Some(glyph) = glyph_for(c) else { continue };
        let advance = glyph.x_advance as i32 * SCALE;
        if cursor_x + advance > panel::WIDTH as i32 - MARGIN {
            cursor_x = start_x;
            cursor_y += line_height;
            if cursor_y > max_y {
                return cursor_y + line_height;
            }
        }
        draw_glyph(display, cursor_x, cursor_y, glyph);
        cursor_x += advance;
    }
    cursor_y + line_height
}

// SCK=11, MOSI=12, DC=5, CS=40, RST=41 -- identical to `main.rs` and to
// `panic_screen_probe.rs`, which proved this exact construction sequence
// works from inside a panic handler.
pub(super) fn build_panel<'d>(
    peripherals_spi2: esp_hal::peripherals::SPI2<'d>,
    sck: esp_hal::peripherals::GPIO11<'d>,
    mosi: esp_hal::peripherals::GPIO12<'d>,
    dc: esp_hal::peripherals::GPIO5<'d>,
    cs: esp_hal::peripherals::GPIO40<'d>,
    rst: esp_hal::peripherals::GPIO41<'d>,
    framebuffer: &'d mut [u8; panel::FRAMEBUFFER_BYTES],
) -> panel::St7305<'d> {
    let spi = Spi::new(
        peripherals_spi2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(24))
            .with_mode(SpiMode::_0),
    )
    .expect("spi2")
    .with_sck(sck)
    .with_mosi(mosi);
    let output = OutputConfig::default();
    let idle_high_output = OutputConfig::default().with_pull(Pull::Up);
    panel::St7305::new(
        framebuffer,
        spi,
        Output::new(dc, Level::Low, output),
        Output::new(cs, Level::High, idle_high_output),
        Output::new(rst, Level::High, idle_high_output),
    )
}
