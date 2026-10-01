mod async_impl;

use super::{
    partial_transition::{
        partial_transition_stats, prepare_partial_transition, prepare_partial_transition_suffix,
        reverse_scan_row_span_for_changes, PartialRowSpan, PartialTransitionStats,
    },
    waveform::{PANEL_PARTIAL_BOUNDED_TRANSITION_PREP, PANEL_PARTIAL_ROW_SPAN_GUARD_ROWS},
    FRAMEBUFFER_BYTES, LUTB, LUTW, PARTIAL_TRANSITION_BYTES,
};

fn panel_partial_row_span(
    previous: &[u8; FRAMEBUFFER_BYTES],
    current: &[u8; FRAMEBUFFER_BYTES],
) -> Option<PartialRowSpan> {
    let span = reverse_scan_row_span_for_changes(previous, current, super::E_INK_WIDTH / 8)?;
    Some(span.widened_by(PANEL_PARTIAL_ROW_SPAN_GUARD_ROWS))
}

fn prepare_panel_partial_transition(
    previous: &[u8; FRAMEBUFFER_BYTES],
    current: &[u8; FRAMEBUFFER_BYTES],
    transition: &mut [u8; PARTIAL_TRANSITION_BYTES],
) {
    prepare_partial_transition(previous, current, transition, &LUTW, &LUTB)
}

fn prepare_panel_partial_transition_for_span(
    previous: &[u8; FRAMEBUFFER_BYTES],
    current: &[u8; FRAMEBUFFER_BYTES],
    transition: &mut [u8; PARTIAL_TRANSITION_BYTES],
    row_span: PartialRowSpan,
) {
    let source_bytes = row_span.scan_rows * (super::E_INK_WIDTH / 8);
    if PANEL_PARTIAL_BOUNDED_TRANSITION_PREP {
        prepare_partial_transition_suffix(previous, current, transition, source_bytes, &LUTW, &LUTB)
    } else {
        prepare_partial_transition(previous, current, transition, &LUTW, &LUTB)
    }
}

fn panel_partial_transition_stats(
    previous: &[u8; FRAMEBUFFER_BYTES],
    current: &[u8; FRAMEBUFFER_BYTES],
) -> PartialTransitionStats {
    partial_transition_stats(previous, current)
}
