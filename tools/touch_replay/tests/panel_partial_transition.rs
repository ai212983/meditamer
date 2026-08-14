#[path = "../../../src/platform/inkplate/partial_transition.rs"]
mod partial_transition;

use partial_transition::{
    partial_transition_stats, prepare_partial_transition, reverse_scan_row_count_for_changes,
};

const LUTW: [u8; 16] = [
    0xFF, 0xFE, 0xFB, 0xFA, 0xEF, 0xEE, 0xEB, 0xEA, 0xBF, 0xBE, 0xBB, 0xBA, 0xAF, 0xAE, 0xAB, 0xAA,
];
const LUTB: [u8; 16] = [
    0xFF, 0xFD, 0xF7, 0xF5, 0xDF, 0xDD, 0xD7, 0xD5, 0x7F, 0x7D, 0x77, 0x75, 0x5F, 0x5D, 0x57, 0x55,
];

#[test]
fn transition_frame_matches_reference_reverse_scan_order() {
    let previous = [0b1010_0101, 0b0000_1111];
    let current = [0b0101_1010, 0b1111_0000];
    let mut transition = [0u8; 4];

    prepare_partial_transition(&previous, &current, &mut transition, &LUTW, &LUTB);
    let stats = partial_transition_stats(&previous, &current);

    assert_eq!(transition, [0x66, 0x99, 0xAA, 0x55]);
    assert_eq!(stats.changed_bytes, 2);
    assert_eq!(stats.changed_pixels, 16);
}

#[test]
fn unchanged_pixels_produce_no_drive_words() {
    let previous = [0x00, 0xA5, 0xFF];
    let mut transition = [0u8; 6];

    prepare_partial_transition(&previous, &previous, &mut transition, &LUTW, &LUTB);
    let stats = partial_transition_stats(&previous, &previous);

    assert_eq!(transition, [0xFF; 6]);
    assert_eq!(stats.changed_bytes, 0);
    assert_eq!(stats.changed_pixels, 0);
}

#[test]
fn transition_stats_count_only_changed_bits() {
    let previous = [0b0000_0000, 0b1111_0000, 0b1010_1010];
    let current = [0b0000_0001, 0b0000_0000, 0b1010_1010];
    let mut transition = [0u8; 6];

    prepare_partial_transition(&previous, &current, &mut transition, &LUTW, &LUTB);
    let stats = partial_transition_stats(&previous, &current);

    assert_eq!(stats.changed_bytes, 2);
    assert_eq!(stats.changed_pixels, 5);
}

#[test]
fn reverse_scan_rows_stop_after_last_required_gate_row() {
    let previous = [0u8; 16];
    let mut current = previous;

    current[14] = 1;
    assert_eq!(
        reverse_scan_row_count_for_changes(&previous, &current, 4),
        Some(1)
    );

    current = previous;
    current[9] = 1;
    assert_eq!(
        reverse_scan_row_count_for_changes(&previous, &current, 4),
        Some(2)
    );

    current = previous;
    current[0] = 1;
    assert_eq!(
        reverse_scan_row_count_for_changes(&previous, &current, 4),
        Some(4)
    );
}

#[test]
fn reverse_scan_rows_skip_refresh_when_framebuffers_match() {
    let framebuffer = [0xA5; 16];
    assert_eq!(
        reverse_scan_row_count_for_changes(&framebuffer, &framebuffer, 4),
        None
    );
}
