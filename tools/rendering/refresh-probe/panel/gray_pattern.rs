//! Native 3-bit gray fixture: levels ramp, gradient, and checkerboard.
//!
//! The reference pattern is deterministic and hash-pinned, so a PSRAM bit
//! flip or a fixture regression halts the soak instead of timing a wrong
//! image. Each timed sample redraws the same base and stamps its marker on
//! top; the hashes in the log distinguish the two layers.

use embassy_time::Instant;
use inkplate_tempera::{E_INK_HEIGHT, E_INK_WIDTH, GRAYSCALE_FRAMEBUFFER_BYTES};

use super::{
    marker::{DIGIT_HEIGHT, DIGIT_WIDTH, MARKER_DIGITS, MARKER_PADDING, MARKER_SCALE},
    resources::{allocate_psram_buffer, halt},
};
use crate::probe_config::{self, DIGIT_ROWS};

pub(crate) fn prepare_gray3_framebuffer() -> &'static mut [u8] {
    let framebuffer = allocate_psram_buffer("gray3", GRAYSCALE_FRAMEBUFFER_BYTES);
    fill_gray3_reference_pattern(framebuffer);
    let pattern_hash = fnv1a(framebuffer);
    console::println!(
        "PANEL_SOAK event=draw phase=gray3_reference display_mode=3bit framebuffer_bytes={} placement=Psram pattern=levels_ramp_boundaries pattern_hash=0x{:08x} expected_hash=0x{:08x} timed=0",
        framebuffer.len(),
        pattern_hash,
        probe_config::GRAY_EXPECTED_FNV1A,
    );
    if pattern_hash != probe_config::GRAY_EXPECTED_FNV1A {
        console::println!(
            "PANEL_SOAK event=halt phase=gray3_reference stage=pattern_hash actual=0x{:08x} expected=0x{:08x}",
            pattern_hash,
            probe_config::GRAY_EXPECTED_FNV1A,
        );
        halt()
    }
    framebuffer
}

pub(crate) fn fill_gray3_reference_pattern(framebuffer: &mut [u8]) {
    assert_eq!(framebuffer.len(), GRAYSCALE_FRAMEBUFFER_BYTES);
    let row_bytes = E_INK_WIDTH / 2;
    for (index, packed) in framebuffer.iter_mut().enumerate() {
        let y = index / row_bytes;
        let x = (index % row_bytes) * 2;
        // The panel presents packed framebuffer coordinates 90 degrees
        // counterclockwise. Store the logical fixture rotated clockwise so it
        // is upright on the physical screen.
        *packed = (gray3_at(y, E_INK_HEIGHT - 1 - x) << 4) | gray3_at(y, E_INK_HEIGHT - 2 - x);
    }
}

pub(crate) fn set_gray4_pixel(framebuffer: &mut [u8], x: usize, y: usize, level: u8) {
    let row_bytes = E_INK_WIDTH / 2;
    let framebuffer_x = E_INK_HEIGHT - 1 - y;
    let framebuffer_y = x;
    let index = framebuffer_y * row_bytes + framebuffer_x / 2;
    if framebuffer_x & 1 == 0 {
        framebuffer[index] = (framebuffer[index] & 0x0f) | (level << 4);
    } else {
        framebuffer[index] = (framebuffer[index] & 0xf0) | level;
    }
}

pub(crate) fn fill_gray4_rect(
    framebuffer: &mut [u8],
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    level: u8,
) {
    for row in y..y + height {
        for column in x..x + width {
            set_gray4_pixel(framebuffer, column, row, level);
        }
    }
}

pub(crate) fn draw_gray3_marker(framebuffer: &mut [u8], marker: u32) {
    let digit_advance = (DIGIT_WIDTH + 1) * MARKER_SCALE;
    let content_width = MARKER_DIGITS * digit_advance - MARKER_SCALE;
    let content_height = DIGIT_HEIGHT * MARKER_SCALE;
    let badge_width = content_width + MARKER_PADDING * 2;
    let badge_height = content_height + MARKER_PADDING * 2;
    let badge_x = (E_INK_WIDTH - badge_width) / 2;
    let badge_y = (E_INK_HEIGHT - badge_height) / 2;

    fill_gray4_rect(framebuffer, badge_x, badge_y, badge_width, badge_height, 15);
    fill_gray4_rect(framebuffer, badge_x, badge_y, badge_width, 1, 0);
    fill_gray4_rect(
        framebuffer,
        badge_x,
        badge_y + badge_height - 1,
        badge_width,
        1,
        0,
    );
    fill_gray4_rect(framebuffer, badge_x, badge_y, 1, badge_height, 0);
    fill_gray4_rect(
        framebuffer,
        badge_x + badge_width - 1,
        badge_y,
        1,
        badge_height,
        0,
    );

    let marker = marker % 1_000;
    let digits = [
        (marker / 100) as usize,
        ((marker / 10) % 10) as usize,
        (marker % 10) as usize,
    ];
    for (digit_index, digit) in digits.into_iter().enumerate() {
        let origin_x = badge_x + MARKER_PADDING + digit_index * digit_advance;
        let origin_y = badge_y + MARKER_PADDING;
        for (row, bits) in DIGIT_ROWS[digit].into_iter().enumerate() {
            for column in 0..DIGIT_WIDTH {
                if bits & (1 << (DIGIT_WIDTH - 1 - column)) != 0 {
                    fill_gray4_rect(
                        framebuffer,
                        origin_x + column * MARKER_SCALE,
                        origin_y + row * MARKER_SCALE,
                        MARKER_SCALE,
                        MARKER_SCALE,
                        0,
                    );
                }
            }
        }
    }
}

pub(crate) fn prepare_numbered_gray3_pattern(
    framebuffer: &mut [u8],
    phase: &str,
    sample: u32,
    marker: u32,
) -> u32 {
    let started = Instant::now();
    fill_gray3_reference_pattern(framebuffer);
    let base_pattern_hash = fnv1a(framebuffer);
    if base_pattern_hash != probe_config::GRAY_EXPECTED_FNV1A {
        console::println!(
            "PANEL_SOAK event=halt phase={} sample={} stage=base_pattern_hash actual=0x{:08x} expected=0x{:08x}",
            phase,
            sample,
            base_pattern_hash,
            probe_config::GRAY_EXPECTED_FNV1A,
        );
        halt()
    }
    draw_gray3_marker(framebuffer, marker);
    let pattern_hash = fnv1a(framebuffer);
    console::println!(
        "PANEL_SOAK event=draw phase={} display_mode=3bit sample={} marker={:03} framebuffer_bytes={} placement=Psram pattern=levels_ramp_boundaries_numbered base_pattern_hash=0x{:08x} pattern_hash=0x{:08x} elapsed_us={} timed=0",
        phase,
        sample,
        marker,
        framebuffer.len(),
        base_pattern_hash,
        pattern_hash,
        started.elapsed().as_micros(),
    );
    pattern_hash
}

pub(crate) fn gray3_at(x: usize, y: usize) -> u8 {
    let band_height = probe_config::GRAY_BAND_HEIGHT as usize;
    if y < band_height {
        let physical_level = (x * 8 / E_INK_WIDTH).min(7);
        return (physical_level * 2) as u8;
    }
    if y < band_height * 2 {
        return (x * 15 / (E_INK_WIDTH - 1)) as u8;
    }

    let checker_tile = probe_config::GRAY_CHECKER_TILE as usize;
    let checker = ((x / checker_tile) + ((y - band_height * 2) / checker_tile)) & 1;
    if x < E_INK_WIDTH / 2 {
        if checker == 0 {
            0
        } else {
            15
        }
    } else if checker == 0 {
        6
    } else {
        8
    }
}

pub(crate) fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}
