use super::super::super::super::config::{SCREEN_HEIGHT, SCREEN_WIDTH};
use super::super::super::config::{TOUCH_WIZARD_SESSION_EVENTS, TOUCH_WIZARD_SWIPE_TRACE_SAMPLES};
use super::super::super::types::{TouchWizardSessionEvent, TouchWizardSwipeTraceSample};
use super::draw::{continue_button_bounds, swipe_mark_button_bounds};
use super::swipe::{
    clamp_to_u16, squared_distance_i32, trace_direction_code, trace_speed_code, SwipeCaseTraceInput,
};
use super::*;

impl TouchCalibrationWizard {
    pub(super) fn shows_swipe_debug(&self) -> bool {
        matches!(self.phase, WizardPhase::SwipeRight | WizardPhase::Complete)
    }

    pub(super) fn shows_continue_button(&self) -> bool {
        !matches!(self.phase, WizardPhase::Closed)
    }

    pub(super) fn continue_button_label(&self) -> &'static str {
        match self.phase {
            WizardPhase::SwipeRight => "SKIP CASE",
            WizardPhase::Complete => "EXIT",
            _ => "CONTINUE",
        }
    }

    pub(super) fn shows_swipe_mark_button(&self) -> bool {
        matches!(self.phase, WizardPhase::SwipeRight)
    }

    pub(super) fn continue_button_hit(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        let (left, top, w, h) = continue_button_bounds(width, height);
        x >= left && x < left + w && y >= top && y < top + h
    }

    pub(super) fn swipe_mark_button_hit(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        let (left, top, w, h) = swipe_mark_button_bounds(width, height);
        x >= left && x < left + w && y >= top && y < top + h
    }

    pub(super) fn on_manual_swipe_mark(&mut self, t_ms: u64) -> bool {
        let prev_hint = self.hint;
        let prev_last_swipe = self.last_swipe;
        let prev_case_failed = self.swipe_case_failed;
        let prev_case_attempts = self.swipe_case_attempts;
        let prev_manual_swipe_marks = self.manual_swipe_marks;

        if matches!(self.phase, WizardPhase::SwipeRight) {
            let case_index = self.swipe_case_index;
            let case = self.current_swipe_case(SCREEN_WIDTH, SCREEN_HEIGHT);
            let start = self.swipe_debug.last_start;
            let end = self.swipe_debug.last_end;
            self.swipe_case_attempts = self.swipe_case_attempts.saturating_add(1);
            self.swipe_case_failed = self.swipe_case_failed.saturating_add(1);
            self.manual_swipe_marks = self.manual_swipe_marks.saturating_add(1);
            self.last_swipe = Some(SwipeAttempt {
                start,
                end,
                accepted: false,
            });
            self.emit_swipe_case_trace(SwipeCaseTraceInput {
                t_ms,
                case_index,
                case,
                verdict: TRACE_VERDICT_MANUAL_MARK,
                classified_direction: None,
                start,
                end,
                duration_ms: self.swipe_debug.last_duration_ms,
                move_count: self.swipe_debug.last_move_count,
                max_travel_px: self.swipe_debug.last_max_travel_px,
                release_debounce_ms: self.swipe_debug.last_release_debounce_ms,
                dropout_count: self.swipe_debug.last_dropout_count,
            });
            self.advance_swipe_case_or_complete(
                t_ms,
                "Manual swipe mark recorded. Next case.",
                "Manual swipe mark recorded. Cases done. Press CONTINUE.",
            );
        }

        self.hint != prev_hint
            || self.last_swipe != prev_last_swipe
            || self.swipe_case_failed != prev_case_failed
            || self.swipe_case_attempts != prev_case_attempts
            || self.manual_swipe_marks != prev_manual_swipe_marks
    }

    pub(super) fn on_continue_button(&mut self, t_ms: u64) -> bool {
        let prev_phase = self.phase;
        let prev_hint = self.hint;
        let prev_last_tap = self.last_tap;
        let prev_swipe_trace = self.swipe_trace;
        let prev_last_swipe = self.last_swipe;

        match self.phase {
            WizardPhase::Intro => {
                self.phase = WizardPhase::TapCenter;
                self.hint = "Manual continue: step 1.";
            }
            WizardPhase::TapCenter => {
                self.phase = WizardPhase::TapTopLeft;
                self.hint = "Manual continue: step 2.";
                self.last_tap = None;
            }
            WizardPhase::TapTopLeft => {
                self.phase = WizardPhase::TapBottomRight;
                self.hint = "Manual continue: step 3.";
                self.last_tap = None;
            }
            WizardPhase::TapBottomRight => {
                self.enter_swipe_phase(t_ms, "Manual continue: guided swipes.");
            }
            WizardPhase::SwipeRight => {
                let case_index = self.swipe_case_index;
                let case = self.current_swipe_case(SCREEN_WIDTH, SCREEN_HEIGHT);
                let (start, end, duration_ms) = if let Some(last) = self.last_swipe {
                    (last.start, last.end, self.swipe_debug.last_duration_ms)
                } else {
                    (
                        self.swipe_debug.last_start,
                        self.swipe_debug.last_end,
                        self.swipe_debug.last_duration_ms,
                    )
                };
                self.emit_swipe_case_trace(SwipeCaseTraceInput {
                    t_ms,
                    case_index,
                    case,
                    verdict: TRACE_VERDICT_SKIP,
                    classified_direction: None,
                    start,
                    end,
                    duration_ms,
                    move_count: self.swipe_debug.last_move_count,
                    max_travel_px: self.swipe_debug.last_max_travel_px,
                    release_debounce_ms: self.swipe_debug.last_release_debounce_ms,
                    dropout_count: self.swipe_debug.last_dropout_count,
                });
                self.advance_swipe_case_or_complete(
                    t_ms,
                    "Case skipped. Next case.",
                    "Swipe cases done. Press CONTINUE to exit.",
                );
            }
            WizardPhase::Complete => {
                self.phase = WizardPhase::Closed;
            }
            WizardPhase::Closed => {}
        }

        self.phase != prev_phase
            || self.hint != prev_hint
            || self.last_tap != prev_last_tap
            || self.swipe_trace != prev_swipe_trace
            || self.last_swipe != prev_last_swipe
    }

    pub(super) fn tap_hits_target(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        let Some((tx, ty)) = self.target_point(width, height) else {
            return false;
        };
        squared_distance_i32(x, y, tx, ty) <= TARGET_HIT_RADIUS_PX * TARGET_HIT_RADIUS_PX
    }

    pub(super) fn enter_swipe_phase(&mut self, t_ms: u64, hint: &'static str) {
        self.phase = WizardPhase::SwipeRight;
        self.hint = hint;
        self.last_tap = None;
        self.clear_swipe_debug();
        self.emit_swipe_session_event(TouchWizardSessionEvent::Start { t_ms });
    }

    pub(super) fn emit_swipe_session_event(&self, event: TouchWizardSessionEvent) {
        let _ = TOUCH_WIZARD_SESSION_EVENTS.try_send(event);
    }

    pub(super) fn emit_swipe_case_trace(&self, trace: SwipeCaseTraceInput) {
        let (expected_direction, expected_speed) = if let Some(spec) = trace.case {
            (
                trace_direction_code(spec.direction),
                trace_speed_code(spec.speed),
            )
        } else {
            (TRACE_DIRECTION_UNKNOWN, TRACE_SPEED_UNKNOWN)
        };
        let sample = TouchWizardSwipeTraceSample {
            t_ms: trace.t_ms,
            case_index: trace.case_index,
            attempt: self.swipe_case_attempts,
            expected_direction,
            expected_speed,
            verdict: trace.verdict,
            classified_direction: trace
                .classified_direction
                .map(trace_direction_code)
                .unwrap_or(TRACE_DIRECTION_UNKNOWN),
            start_x: clamp_to_u16(trace.start.x),
            start_y: clamp_to_u16(trace.start.y),
            end_x: clamp_to_u16(trace.end.x),
            end_y: clamp_to_u16(trace.end.y),
            duration_ms: trace.duration_ms,
            move_count: trace.move_count,
            max_travel_px: trace.max_travel_px,
            release_debounce_ms: trace.release_debounce_ms,
            dropout_count: trace.dropout_count,
        };
        let _ = TOUCH_WIZARD_SWIPE_TRACE_SAMPLES.try_send(sample);
    }

    pub(super) fn step_progress_text(&self) -> &'static str {
        match self.phase {
            WizardPhase::Intro => "Step 0/4",
            WizardPhase::TapCenter => "Step 1/4",
            WizardPhase::TapTopLeft => "Step 2/4",
            WizardPhase::TapBottomRight => "Step 3/4",
            WizardPhase::SwipeRight => "Step 4/4 Guided Swipes",
            WizardPhase::Complete => "Done",
            WizardPhase::Closed => "",
        }
    }

    pub(super) fn primary_instruction(&self) -> &'static str {
        match self.phase {
            WizardPhase::Intro => "Tap anywhere to begin touch checks.",
            WizardPhase::TapCenter => "Tap the center target.",
            WizardPhase::TapTopLeft => "Tap the top-left target.",
            WizardPhase::TapBottomRight => "Tap the bottom-right target.",
            WizardPhase::SwipeRight => "Perform the guided swipe case.",
            WizardPhase::Complete => "Calibration complete.",
            WizardPhase::Closed => "",
        }
    }

    pub(super) fn secondary_instruction(&self) -> &'static str {
        match self.phase {
            WizardPhase::Intro => "This validates tap and swipe tracking.",
            WizardPhase::TapCenter => "Aim inside the ring.",
            WizardPhase::TapTopLeft => "Aim inside the ring.",
            WizardPhase::TapBottomRight => "Aim inside the ring.",
            WizardPhase::SwipeRight => {
                "FROM->TO + direction. Speed logged. Use I JUST SWIPED or SKIP CASE."
            }
            WizardPhase::Complete => "Exit with the EXIT button.",
            WizardPhase::Closed => "",
        }
    }

    pub(super) fn target_point(&self, width: i32, height: i32) -> Option<(i32, i32)> {
        let w = width.max(1);
        let h = height.max(1);
        match self.phase {
            WizardPhase::TapCenter => Some((w / 2, h / 2 + 24)),
            WizardPhase::TapTopLeft => Some((w / 5, h / 3)),
            WizardPhase::TapBottomRight => Some((w * 4 / 5, h * 2 / 3)),
            _ => None,
        }
    }
}
