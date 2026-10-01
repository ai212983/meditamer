use super::*;

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
fn suffix_preparation_matches_full_frame_and_preserves_prefix() {
    let previous = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
    let current = [0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA, 0x99, 0x88];
    let mut full = [0u8; 16];
    prepare_partial_transition(&previous, &current, &mut full, &LUTW, &LUTB);

    for source_bytes in 0..=previous.len() {
        let transition_start = (previous.len() - source_bytes) * 2;
        let mut suffix = [0xA5; 16];
        prepare_partial_transition_suffix(
            &previous,
            &current,
            &mut suffix,
            source_bytes,
            &LUTW,
            &LUTB,
        );
        assert_eq!(&suffix[..transition_start], &[0xA5; 16][..transition_start]);
        assert_eq!(&suffix[transition_start..], &full[transition_start..]);
    }
}

#[test]
fn reverse_scan_span_reports_current_work_and_two_sided_skip_candidate() {
    let previous = [0u8; 16];
    let mut current = previous;

    current[14] = 1;
    assert_eq!(
        reverse_scan_row_span_for_changes(&previous, &current, 4),
        Some(PartialRowSpan {
            scan_rows: 1,
            first_changed_row: 3,
            last_changed_row: 3,
            changed_span_rows: 1,
            source_skip_candidate_rows: 0,
        })
    );

    current = previous;
    current[9] = 1;
    assert_eq!(
        reverse_scan_row_span_for_changes(&previous, &current, 4),
        Some(PartialRowSpan {
            scan_rows: 2,
            first_changed_row: 2,
            last_changed_row: 2,
            changed_span_rows: 1,
            source_skip_candidate_rows: 1,
        })
    );

    current = previous;
    current[4] = 1;
    current[10] = 1;
    assert_eq!(
        reverse_scan_row_span_for_changes(&previous, &current, 4),
        Some(PartialRowSpan {
            scan_rows: 3,
            first_changed_row: 1,
            last_changed_row: 2,
            changed_span_rows: 2,
            source_skip_candidate_rows: 1,
        })
    );
}

#[test]
fn widened_by_grows_scan_rows_without_disturbing_the_changed_span() {
    let previous = [0u8; 16];
    let mut current = previous;
    current[9] = 1;
    let span = reverse_scan_row_span_for_changes(&previous, &current, 4)
        .unwrap()
        .widened_by(1);
    assert_eq!(
        span,
        PartialRowSpan {
            scan_rows: 3,
            first_changed_row: 2,
            last_changed_row: 2,
            changed_span_rows: 1,
            source_skip_candidate_rows: 1,
        }
    );
}

#[test]
fn widened_by_clamps_at_the_full_frame() {
    let previous = [0u8; 16];
    let mut current = previous;
    current[9] = 1;
    let span = reverse_scan_row_span_for_changes(&previous, &current, 4)
        .unwrap()
        .widened_by(100);
    assert_eq!(span.scan_rows, 4);
}

#[test]
fn reverse_scan_rows_skip_refresh_when_framebuffers_match() {
    let framebuffer = [0xA5; 16];
    assert_eq!(
        reverse_scan_row_span_for_changes(&framebuffer, &framebuffer, 4),
        None
    );
}

#[test]
fn reverse_scan_span_preserves_exact_bytes_across_chunk_tail() {
    let previous = [0u8; 15];
    for first_changed_byte in 0..previous.len() {
        for last_changed_byte in first_changed_byte..previous.len() {
            let mut current = previous;
            current[first_changed_byte] = 1;
            current[last_changed_byte] = 2;

            let first_changed_row = first_changed_byte / 3;
            let last_changed_row = last_changed_byte / 3;
            assert_eq!(
                reverse_scan_row_span_for_changes(&previous, &current, 3),
                Some(PartialRowSpan {
                    scan_rows: 5 - first_changed_row,
                    first_changed_row,
                    last_changed_row,
                    changed_span_rows: last_changed_row - first_changed_row + 1,
                    source_skip_candidate_rows: 5 - last_changed_row - 1,
                })
            );
        }
    }
}
