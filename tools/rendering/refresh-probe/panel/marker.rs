//! Numbered-marker primitives shared by every 1-bit fixture.
//!
//! Each refresh carries a centered numeric badge so captures stay attributable
//! to their sample. This module owns the badge geometry, the framebuffer pixel
//! helper, and the driver-level marker painters built on them.

use inkplate_tempera::{adapters::BusyDelay, InkplateHal, TestPattern, E_INK_HEIGHT, E_INK_WIDTH};

use super::resources::ProbeI2cDevice;
use crate::probe_config::{self, DIGIT_ROWS};

pub(crate) const MARKER_SCALE: usize = probe_config::MARKER_SCALE as usize;
pub(crate) const MARKER_DIGITS: usize = probe_config::MARKER_DIGITS as usize;
pub(crate) const DIGIT_WIDTH: usize = probe_config::DIGIT_WIDTH as usize;
pub(crate) const DIGIT_HEIGHT: usize = probe_config::DIGIT_HEIGHT as usize;
pub(crate) const MARKER_PADDING: usize = probe_config::MARKER_PADDING as usize;
pub(crate) const SPAN_MARKER_SCALE: usize = 4;
pub(crate) const SPAN_MARKER_PADDING: usize = 4;

pub(crate) fn draw_marker(driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>, marker: u32) {
    let digit_advance = (DIGIT_WIDTH + 1) * MARKER_SCALE;
    let content_width = MARKER_DIGITS * digit_advance - MARKER_SCALE;
    let content_height = DIGIT_HEIGHT * MARKER_SCALE;
    let badge_width = content_width + MARKER_PADDING * 2;
    let badge_height = content_height + MARKER_PADDING * 2;
    let badge_x = (E_INK_WIDTH - badge_width) / 2;
    let badge_y = (E_INK_HEIGHT - badge_height) / 2;

    for y in badge_y..badge_y + badge_height {
        for x in badge_x..badge_x + badge_width {
            let border = x == badge_x
                || x == badge_x + badge_width - 1
                || y == badge_y
                || y == badge_y + badge_height - 1;
            driver.set_pixel_bw(x, y, border);
        }
    }

    let value = visible_marker(marker);
    let digits = [value / 100, (value / 10) % 10, value % 10];
    for (digit_index, digit) in digits.into_iter().enumerate() {
        let origin_x = badge_x + MARKER_PADDING + digit_index * digit_advance;
        let origin_y = badge_y + MARKER_PADDING;
        for (row, bits) in DIGIT_ROWS[digit as usize].into_iter().enumerate() {
            for column in 0..DIGIT_WIDTH {
                if bits & (1 << (DIGIT_WIDTH - 1 - column)) == 0 {
                    continue;
                }
                let pixel_x = origin_x + column * MARKER_SCALE;
                let pixel_y = origin_y + row * MARKER_SCALE;
                for scaled_y in pixel_y..pixel_y + MARKER_SCALE {
                    for scaled_x in pixel_x..pixel_x + MARKER_SCALE {
                        driver.set_pixel_bw(scaled_x, scaled_y, true);
                    }
                }
            }
        }
    }
}

pub(crate) fn draw_span_marker(
    framebuffer: &mut [u8],
    first_changed_row: usize,
    scan_rows: usize,
    marker: u32,
) {
    let digit_advance = (DIGIT_WIDTH + 1) * SPAN_MARKER_SCALE;
    let content_width = MARKER_DIGITS * digit_advance - SPAN_MARKER_SCALE;
    let content_height = DIGIT_HEIGHT * SPAN_MARKER_SCALE;
    let badge_width = content_width + SPAN_MARKER_PADDING * 2;
    let badge_height = content_height + SPAN_MARKER_PADDING * 2;
    debug_assert!(badge_height <= scan_rows);
    let badge_x = (E_INK_WIDTH - badge_width) / 2;
    let badge_y = first_changed_row + (scan_rows - badge_height) / 2;

    for y in badge_y..badge_y + badge_height {
        for x in badge_x..badge_x + badge_width {
            let border = x == badge_x
                || x == badge_x + badge_width - 1
                || y == badge_y
                || y == badge_y + badge_height - 1;
            set_panel_pixel_bw(framebuffer, x, y, border);
        }
    }

    let digits = [marker / 100, (marker / 10) % 10, marker % 10];
    for (digit_index, digit) in digits.into_iter().enumerate() {
        let origin_x = badge_x + SPAN_MARKER_PADDING + digit_index * digit_advance;
        let origin_y = badge_y + SPAN_MARKER_PADDING;
        for (row, bits) in DIGIT_ROWS[digit as usize].into_iter().enumerate() {
            for column in 0..DIGIT_WIDTH {
                if bits & (1 << (DIGIT_WIDTH - 1 - column)) == 0 {
                    continue;
                }
                let pixel_x = origin_x + column * SPAN_MARKER_SCALE;
                let pixel_y = origin_y + row * SPAN_MARKER_SCALE;
                for scaled_y in pixel_y..pixel_y + SPAN_MARKER_SCALE {
                    for scaled_x in pixel_x..pixel_x + SPAN_MARKER_SCALE {
                        set_panel_pixel_bw(framebuffer, scaled_x, scaled_y, true);
                    }
                }
            }
        }
    }
}

pub(crate) fn set_panel_pixel_bw(framebuffer: &mut [u8], x: usize, y: usize, black: bool) {
    debug_assert!(x < E_INK_WIDTH && y < E_INK_HEIGHT);
    let byte_index = (E_INK_WIDTH / 8) * y + x / 8;
    let bit = 1u8 << (x % 8);
    if black {
        framebuffer[byte_index] |= bit;
    } else {
        framebuffer[byte_index] &= !bit;
    }
}

pub(crate) const fn visible_marker(value: u32) -> u32 {
    value % 1_000
}

pub(crate) const fn alternating_pattern(cycle: u32) -> TestPattern {
    if cycle.is_multiple_of(2) {
        TestPattern::VerticalBars
    } else {
        TestPattern::HorizontalBars
    }
}
