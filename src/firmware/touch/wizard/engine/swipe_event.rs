use super::super::super::super::config::{SCREEN_HEIGHT, SCREEN_WIDTH};
use super::super::super::types::{TouchEvent, TouchSwipeDirection};
use super::events::pending_release_matches_swipe;
use super::swipe::{swipe_case_matches, swipe_start_matches, SwipeCaseTraceInput};
use super::*;

impl TouchCalibrationWizard {
    pub(super) fn on_swipe_event(
        &mut self,
        event: TouchEvent,
        direction: TouchSwipeDirection,
    ) -> bool {
        let prev_phase = self.phase;
        let prev_hint = self.hint;
        let prev_last_tap = self.last_tap;
        let prev_last_swipe = self.last_swipe;
        let prev_case_index = self.swipe_case_index;
        let prev_case_passed = self.swipe_case_passed;
        let prev_case_failed = self.swipe_case_failed;
        let prev_case_attempts = self.swipe_case_attempts;
        if self.phase == WizardPhase::SwipeRight {
            let case_index = self.swipe_case_index;
            let case = self.current_swipe_case(SCREEN_WIDTH, SCREEN_HEIGHT);
            let start = SwipePoint {
                x: event.start_x as i32,
                y: event.start_y as i32,
            };
            let end = SwipePoint {
                x: event.x as i32,
                y: event.y as i32,
            };
            self.append_swipe_trace_point(event.x as i32, event.y as i32);
            if case.is_some_and(|spec| !swipe_start_matches(spec, start)) {
                self.hint = "Start outside FROM circle. Retry this case.";
                self.last_swipe = Some(SwipeAttempt {
                    start,
                    end,
                    accepted: false,
                });
                self.swipe_trace_pending_points = 0;
                self.emit_swipe_case_trace(SwipeCaseTraceInput {
                    t_ms: event.t_ms,
                    case_index,
                    case,
                    verdict: TRACE_VERDICT_SKIP,
                    classified_direction: Some(direction),
                    start,
                    end,
                    duration_ms: event.duration_ms,
                    move_count: event.move_count,
                    max_travel_px: event.max_travel_px,
                    release_debounce_ms: event.release_debounce_ms,
                    dropout_count: event.dropout_count,
                });
                return self.phase != prev_phase
                    || self.hint != prev_hint
                    || self.last_tap != prev_last_tap
                    || self.last_swipe != prev_last_swipe
                    || self.swipe_case_index != prev_case_index
                    || self.swipe_case_passed != prev_case_passed
                    || self.swipe_case_failed != prev_case_failed
                    || self.swipe_case_attempts != prev_case_attempts;
            }
            self.swipe_case_attempts = self.swipe_case_attempts.saturating_add(1);
            let mut case_pass = false;
            if let Some(case) = case {
                case_pass =
                    swipe_case_matches(case, start, end, event.duration_ms, Some(direction), true);
            }
            self.last_swipe = Some(SwipeAttempt {
                start,
                end,
                accepted: case_pass,
            });
            self.swipe_trace_pending_points = 0;
            if case_pass {
                self.swipe_case_passed = self.swipe_case_passed.saturating_add(1);
                self.emit_swipe_case_trace(SwipeCaseTraceInput {
                    t_ms: event.t_ms,
                    case_index,
                    case,
                    verdict: TRACE_VERDICT_PASS,
                    classified_direction: Some(direction),
                    start,
                    end,
                    duration_ms: event.duration_ms,
                    move_count: event.move_count,
                    max_travel_px: event.max_travel_px,
                    release_debounce_ms: event.release_debounce_ms,
                    dropout_count: event.dropout_count,
                });
                self.advance_swipe_case_or_complete(
                    event.t_ms,
                    "Swipe PASS. Next case.",
                    "All swipe cases done. Press CONTINUE to exit.",
                );
            } else {
                self.swipe_case_failed = self.swipe_case_failed.saturating_add(1);
                self.emit_swipe_case_trace(SwipeCaseTraceInput {
                    t_ms: event.t_ms,
                    case_index,
                    case,
                    verdict: TRACE_VERDICT_MISMATCH,
                    classified_direction: Some(direction),
                    start,
                    end,
                    duration_ms: event.duration_ms,
                    move_count: event.move_count,
                    max_travel_px: event.max_travel_px,
                    release_debounce_ms: event.release_debounce_ms,
                    dropout_count: event.dropout_count,
                });
                self.advance_swipe_case_or_complete(
                    event.t_ms,
                    "Swipe recorded (mismatch). Next case.",
                    "All swipe cases done. Press CONTINUE to exit.",
                );
            }
        }
        self.phase != prev_phase
            || self.hint != prev_hint
            || self.last_tap != prev_last_tap
            || self.last_swipe != prev_last_swipe
            || self.swipe_case_index != prev_case_index
            || self.swipe_case_passed != prev_case_passed
            || self.swipe_case_failed != prev_case_failed
            || self.swipe_case_attempts != prev_case_attempts
    }

    pub(super) fn on_swipe_release(&mut self, event: TouchEvent) -> bool {
        let prev_last_tap = self.last_tap;
        let prev_last_swipe = self.last_swipe;
        let prev_pending_swipe_release = self.pending_swipe_release;

        if matches!(self.phase, WizardPhase::SwipeRight) {
            let start = SwipePoint {
                x: event.start_x as i32,
                y: event.start_y as i32,
            };
            let end = SwipePoint {
                x: event.x as i32,
                y: event.y as i32,
            };
            self.append_swipe_trace_point(event.x as i32, event.y as i32);
            self.last_swipe = Some(SwipeAttempt {
                start,
                end,
                accepted: false,
            });
            self.swipe_trace_pending_points = 0;
            self.pending_swipe_release = Some(PendingSwipeRelease {
                t_ms: event.t_ms,
                start,
                end,
                duration_ms: event.duration_ms,
                move_count: event.move_count,
                max_travel_px: event.max_travel_px,
                release_debounce_ms: event.release_debounce_ms,
                dropout_count: event.dropout_count,
            });
        }

        self.last_tap != prev_last_tap
            || self.last_swipe != prev_last_swipe
            || self.pending_swipe_release != prev_pending_swipe_release
    }

    pub(super) fn resolve_pending_swipe_release(
        &mut self,
        event: TouchEvent,
        continue_hit: bool,
        swipe_mark_hit: bool,
    ) -> bool {
        let Some(pending) = self.pending_swipe_release else {
            return false;
        };

        if continue_hit || swipe_mark_hit {
            self.pending_swipe_release = None;
            return false;
        }

        if pending_release_matches_swipe(pending, event) {
            self.pending_swipe_release = None;
            return false;
        }

        self.pending_swipe_release = None;
        self.commit_swipe_release_no_swipe(pending)
    }

    fn commit_swipe_release_no_swipe(&mut self, pending: PendingSwipeRelease) -> bool {
        let prev_hint = self.hint;
        let prev_last_swipe = self.last_swipe;
        let prev_case_failed = self.swipe_case_failed;
        let prev_case_attempts = self.swipe_case_attempts;

        if matches!(self.phase, WizardPhase::SwipeRight) {
            let case_index = self.swipe_case_index;
            let case = self.current_swipe_case(SCREEN_WIDTH, SCREEN_HEIGHT);
            self.last_swipe = Some(SwipeAttempt {
                start: pending.start,
                end: pending.end,
                accepted: false,
            });
            if case.is_some_and(|spec| !swipe_start_matches(spec, pending.start)) {
                self.hint = "Release outside FROM circle. Retry this case.";
                self.emit_swipe_case_trace(SwipeCaseTraceInput {
                    t_ms: pending.t_ms,
                    case_index,
                    case,
                    verdict: TRACE_VERDICT_SKIP,
                    classified_direction: None,
                    start: pending.start,
                    end: pending.end,
                    duration_ms: pending.duration_ms,
                    move_count: pending.move_count,
                    max_travel_px: pending.max_travel_px,
                    release_debounce_ms: pending.release_debounce_ms,
                    dropout_count: pending.dropout_count,
                });
            } else {
                self.swipe_case_attempts = self.swipe_case_attempts.saturating_add(1);
                self.swipe_case_failed = self.swipe_case_failed.saturating_add(1);
                self.hint = "Release w/o swipe. Retry this case.";
                self.emit_swipe_case_trace(SwipeCaseTraceInput {
                    t_ms: pending.t_ms,
                    case_index,
                    case,
                    verdict: TRACE_VERDICT_RELEASE_NO_SWIPE,
                    classified_direction: None,
                    start: pending.start,
                    end: pending.end,
                    duration_ms: pending.duration_ms,
                    move_count: pending.move_count,
                    max_travel_px: pending.max_travel_px,
                    release_debounce_ms: pending.release_debounce_ms,
                    dropout_count: pending.dropout_count,
                });
            }
        }

        self.hint != prev_hint
            || self.last_swipe != prev_last_swipe
            || self.swipe_case_failed != prev_case_failed
            || self.swipe_case_attempts != prev_case_attempts
    }
}
