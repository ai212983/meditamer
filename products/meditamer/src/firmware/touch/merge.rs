//! Deterministic interleaving order for the two touch input streams.
//!
//! Single-touch events (`TOUCH_PIPELINE_EVENTS`) and LVGL multitouch frames
//! (`TOUCH_LVGL_MULTITOUCH_FRAMES`) are independent bounded queues, but they
//! describe one physical touch surface and must be dispatched to LVGL in the
//! order the controller actually observed them. Draining one queue to
//! exhaustion before touching the other collapses that order into a
//! source-priority order instead, which can deliver a multitouch release
//! before a single-touch press that physically happened first (or vice
//! versa). This module holds only the pure timestamp comparison so the
//! selection is host-testable without the embedded display/backend types.

/// Which time-stamped touch source has the next event to dispatch, given
/// each source's next queued timestamp (`None` when that source's lookahead
/// buffer is empty).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NextTouchSource {
    Single,
    Multitouch,
}

/// Whether a queued single-touch candidate may replace the retained `Move`
/// instead of being dispatched after it. Only `Move` candidates collapse, and
/// only within one admission epoch and strictly before the multitouch
/// lookahead frame. An earlier-or-equal frame ends coalescing; normal stream
/// selection then preserves its existing ordering, including single-touch
/// precedence on equal timestamps. A non-`Move` or cross-epoch candidate ends
/// the run. `candidate_is_move` keeps
/// this module free of the embedded event type so the rule stays host-testable
/// next to [`next_touch_source`].
pub(crate) fn coalesce_move_candidate(
    candidate_is_move: bool,
    retained_epoch: u32,
    candidate_epoch: u32,
    candidate_t_ms: u64,
    next_multitouch_t_ms: Option<u64>,
) -> bool {
    candidate_is_move
        && candidate_epoch == retained_epoch
        && next_multitouch_t_ms.is_none_or(|multi| candidate_t_ms < multi)
}

/// Disposition of one queued single-touch candidate against the local batch
/// being built by the presentation drain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BatchCandidateAction {
    /// The candidate is a `Move` superseded by the batch's trailing `Move`:
    /// overwrite the trailing entry with the candidate (newest wins).
    CollapseIntoLastMove,
    /// Preserve the candidate as a new trailing batch entry. Every non-`Move`
    /// edge (`Down`/`Up`/`Cancel`/tap/hold/swipe) takes this path, as does a
    /// `Move` that opens motion after an edge or under a new admission epoch.
    Retain,
    /// Timestamp-ordered multitouch boundary: the candidate is at or past the
    /// multitouch lookahead frame, so it stays buffered for the normal stream
    /// selector instead of joining this batch.
    Defer,
}

/// Classify one queued single-touch candidate for the presentation batch.
///
/// `candidate_is_move`/`last_is_move` keep this module free of the embedded
/// event type so the rule stays host-testable next to [`next_touch_source`].
/// Terminal handling (`Up`/`Cancel` ends the batch) and capacity live with the
/// caller: a retained terminal event still reports [`BatchCandidateAction::Retain`].
pub(crate) fn batch_candidate_action(
    candidate_is_move: bool,
    last_is_move: bool,
    last_epoch: u32,
    candidate_epoch: u32,
    candidate_t_ms: u64,
    next_multitouch_t_ms: Option<u64>,
) -> BatchCandidateAction {
    if next_multitouch_t_ms.is_some_and(|multi| candidate_t_ms >= multi) {
        return BatchCandidateAction::Defer;
    }
    if candidate_is_move
        && last_is_move
        && coalesce_move_candidate(
            true,
            last_epoch,
            candidate_epoch,
            candidate_t_ms,
            next_multitouch_t_ms,
        )
    {
        return BatchCandidateAction::CollapseIntoLastMove;
    }
    BatchCandidateAction::Retain
}

/// Compare the next queued timestamp from each source and report which one
/// should be dispatched next. A tie resolves to the single-touch source so a
/// recognized tap/hold is never starved behind an equally-timestamped
/// multitouch frame.
pub(crate) fn next_touch_source(
    next_single_t_ms: Option<u64>,
    next_multitouch_t_ms: Option<u64>,
) -> Option<NextTouchSource> {
    match (next_single_t_ms, next_multitouch_t_ms) {
        (None, None) => None,
        (Some(_), None) => Some(NextTouchSource::Single),
        (None, Some(_)) => Some(NextTouchSource::Multitouch),
        (Some(single), Some(multi)) => Some(if single <= multi {
            NextTouchSource::Single
        } else {
            NextTouchSource::Multitouch
        }),
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn empty_sources_yield_nothing() {
        assert_eq!(next_touch_source(None, None), None);
    }

    #[test]
    fn single_only_source_is_selected() {
        assert_eq!(
            next_touch_source(Some(10), None),
            Some(NextTouchSource::Single)
        );
    }

    #[test]
    fn multitouch_only_source_is_selected() {
        assert_eq!(
            next_touch_source(None, Some(10)),
            Some(NextTouchSource::Multitouch)
        );
    }

    #[test]
    fn earlier_timestamp_wins_regardless_of_source() {
        assert_eq!(
            next_touch_source(Some(5), Some(10)),
            Some(NextTouchSource::Single)
        );
        assert_eq!(
            next_touch_source(Some(10), Some(5)),
            Some(NextTouchSource::Multitouch)
        );
    }

    #[test]
    fn tied_timestamps_prefer_single_touch() {
        assert_eq!(
            next_touch_source(Some(7), Some(7)),
            Some(NextTouchSource::Single)
        );
    }

    #[test]
    fn consecutive_same_epoch_moves_coalesce_without_lookahead() {
        assert!(coalesce_move_candidate(true, 3, 3, 10, None));
    }

    #[test]
    fn non_move_candidate_ends_the_run() {
        assert!(!coalesce_move_candidate(false, 3, 3, 10, None));
    }

    #[test]
    fn cross_epoch_move_ends_the_run() {
        assert!(!coalesce_move_candidate(true, 3, 4, 10, None));
    }

    #[test]
    fn move_before_lookahead_coalesces() {
        assert!(coalesce_move_candidate(true, 3, 3, 9, Some(10)));
    }

    #[test]
    fn earlier_or_equal_lookahead_ends_the_run() {
        assert!(!coalesce_move_candidate(true, 3, 3, 10, Some(10)));
        assert!(!coalesce_move_candidate(true, 3, 3, 11, Some(10)));
    }

    #[test]
    fn superseded_move_collapses_into_trailing_move() {
        assert_eq!(
            batch_candidate_action(true, true, 3, 3, 10, None),
            BatchCandidateAction::CollapseIntoLastMove
        );
    }

    #[test]
    fn move_after_edge_opens_a_new_retained_entry() {
        assert_eq!(
            batch_candidate_action(true, false, 3, 3, 10, None),
            BatchCandidateAction::Retain
        );
    }

    #[test]
    fn non_move_edges_are_always_retained() {
        // Down, terminal Up/Cancel, and tap/hold/swipe kinds all arrive here
        // as `candidate_is_move == false`; the caller ends the batch after
        // retaining a terminal event.
        for last_is_move in [false, true] {
            assert_eq!(
                batch_candidate_action(false, last_is_move, 3, 3, 10, None),
                BatchCandidateAction::Retain
            );
        }
    }

    #[test]
    fn terminal_up_reports_retain_for_caller_to_close_the_batch() {
        assert_eq!(
            batch_candidate_action(false, true, 3, 3, 10, None),
            BatchCandidateAction::Retain
        );
    }

    #[test]
    fn cross_epoch_move_is_retained_not_collapsed() {
        assert_eq!(
            batch_candidate_action(true, true, 3, 4, 10, None),
            BatchCandidateAction::Retain
        );
    }

    #[test]
    fn multitouch_boundary_defers_moves_and_edges() {
        // At-or-past the lookahead frame the candidate stays buffered so the
        // timestamp-ordered selector dispatches the multitouch frame first.
        // Equal timestamps defer here even though the selector prefers the
        // single-touch source on a tie: the deferred candidate opens the next
        // batch and still runs before the frame.
        assert_eq!(
            batch_candidate_action(true, true, 3, 3, 10, Some(10)),
            BatchCandidateAction::Defer
        );
        assert_eq!(
            batch_candidate_action(true, true, 3, 3, 11, Some(10)),
            BatchCandidateAction::Defer
        );
        assert_eq!(
            batch_candidate_action(false, false, 3, 3, 10, Some(10)),
            BatchCandidateAction::Defer
        );
        assert_eq!(
            batch_candidate_action(false, true, 3, 3, 9, Some(10)),
            BatchCandidateAction::Retain
        );
    }

    #[test]
    fn move_before_lookahead_collapses_or_retains_by_trailing_entry() {
        assert_eq!(
            batch_candidate_action(true, true, 3, 3, 9, Some(10)),
            BatchCandidateAction::CollapseIntoLastMove
        );
        assert_eq!(
            batch_candidate_action(true, false, 3, 3, 9, Some(10)),
            BatchCandidateAction::Retain
        );
    }
}
