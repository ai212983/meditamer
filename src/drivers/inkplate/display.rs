mod async_impl;

use super::{
    partial_transition::{
        partial_transition_stats, prepare_partial_transition, reverse_scan_row_count_for_changes,
        PartialTransitionStats,
    },
    FRAMEBUFFER_BYTES, LUTB, LUTW, PARTIAL_TRANSITION_BYTES,
};

fn panel_partial_scan_rows(
    previous: &[u8; FRAMEBUFFER_BYTES],
    current: &[u8; FRAMEBUFFER_BYTES],
) -> Option<usize> {
    reverse_scan_row_count_for_changes(previous, current, super::E_INK_WIDTH / 8)
}

fn prepare_panel_partial_transition(
    previous: &[u8; FRAMEBUFFER_BYTES],
    current: &[u8; FRAMEBUFFER_BYTES],
    transition: &mut [u8; PARTIAL_TRANSITION_BYTES],
) {
    prepare_partial_transition(previous, current, transition, &LUTW, &LUTB)
}

fn panel_partial_transition_stats(
    previous: &[u8; FRAMEBUFFER_BYTES],
    current: &[u8; FRAMEBUFFER_BYTES],
) -> PartialTransitionStats {
    partial_transition_stats(previous, current)
}
