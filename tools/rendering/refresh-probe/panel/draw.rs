//! 1-bit fixture painters and the sparse-delta contract check.
//!
//! The partial-then-full soak redraws an invariant base whose only change is
//! the centered counter. The debug snapshot below enforces that sparse-delta
//! contract so the full successor cannot silently widen the logical update.

use inkplate_tempera::{adapters::BusyDelay, InkplateHal, TestPattern, E_INK_HEIGHT, E_INK_WIDTH};

use super::{
    marker::{
        draw_marker, draw_span_marker, DIGIT_HEIGHT, DIGIT_WIDTH, MARKER_DIGITS, MARKER_PADDING,
        MARKER_SCALE,
    },
    probe_mode::should_emit_cycle,
    resources::{halt, ProbeI2cDevice},
};

pub(crate) fn draw_pattern(
    driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>,
    pattern: TestPattern,
    marker: u32,
) {
    driver.draw_test_pattern(pattern);
    draw_marker(driver, marker);
}

pub(crate) fn validate_sparse_center_counter_delta(
    driver: &InkplateHal<ProbeI2cDevice, BusyDelay>,
    cycle: u32,
    marker: u32,
) {
    let digit_advance = (DIGIT_WIDTH + 1) * MARKER_SCALE;
    let content_width = MARKER_DIGITS * digit_advance - MARKER_SCALE;
    let content_height = DIGIT_HEIGHT * MARKER_SCALE;
    let badge_width = content_width + MARKER_PADDING * 2;
    let badge_height = content_height + MARKER_PADDING * 2;
    let badge_x = (E_INK_WIDTH - badge_width) / 2;
    let badge_y = (E_INK_HEIGHT - badge_height) / 2;
    // set_pixel_bw rotates logical pixels clockwise into the backing buffer:
    // backing row = logical x, backing column = width - 1 - logical y.
    // The debug snapshot reports those backing-buffer coordinates.
    let first_badge_row = badge_x;
    let last_badge_row = badge_x + badge_width - 1;
    let first_badge_byte = (E_INK_WIDTH - (badge_y + badge_height)) / 8;
    let last_badge_byte = (E_INK_WIDTH - 1 - badge_y) / 8;

    let snapshot = driver.binary_framebuffer_debug_snapshot();
    let (Some(min_row), Some(max_row), Some(min_byte), Some(max_byte)) = (
        snapshot.min_row,
        snapshot.max_row,
        snapshot.min_byte_column,
        snapshot.max_byte_column,
    ) else {
        console::println!(
            "PANEL_SOAK event=halt mode=PartialThenFull cycle={} phase=partial_predecessor stage=sparse_delta_empty marker={:03} current_hash=0x{:08x} previous_hash={:?}",
            cycle,
            marker,
            snapshot.current_hash,
            snapshot.previous_hash,
        );
        halt()
    };

    if min_row < first_badge_row
        || max_row > last_badge_row
        || min_byte < first_badge_byte
        || max_byte > last_badge_byte
    {
        console::println!(
            "PANEL_SOAK event=halt mode=PartialThenFull cycle={} phase=partial_predecessor stage=sparse_delta_bounds marker={:03} changed_rows={}..{} changed_byte_columns={}..{} expected_rows={}..{} expected_byte_columns={}..{}",
            cycle,
            marker,
            min_row,
            max_row,
            min_byte,
            max_byte,
            first_badge_row,
            last_badge_row,
            first_badge_byte,
            last_badge_byte,
        );
        halt()
    }

    if should_emit_cycle(cycle) {
        console::println!(
            "PANEL_SOAK event=draw mode=PartialThenFull cycle={} phase=partial_predecessor fixture=sparse_center_counter marker={:03} current_hash=0x{:08x} previous_hash={:?} changed_bytes={} changed_pixels={} changed_rows={}..{} changed_byte_columns={}..{} timed=0",
            cycle,
            marker,
            snapshot.current_hash,
            snapshot.previous_hash,
            snapshot.changed_bytes,
            snapshot.changed_pixels,
            min_row,
            max_row,
            min_byte,
            max_byte,
        );
    }
}

pub(crate) fn draw_partial_span_pattern(
    driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>,
    scan_rows: usize,
    sample: u32,
    marker: u32,
) {
    debug_assert!((1..=E_INK_HEIGHT).contains(&scan_rows));
    let first_changed_row = E_INK_HEIGHT - scan_rows;
    let row_bytes = E_INK_WIDTH / 8;
    let fill = if sample % 2 == 1 { 0xFF } else { 0x00 };
    driver.clear_bw();
    let framebuffer = driver.framebuffer_bw_mut();
    framebuffer[first_changed_row * row_bytes..].fill(fill);
    draw_span_marker(framebuffer, first_changed_row, scan_rows, marker);
}
