use super::super::super::super::config::{SCREEN_HEIGHT, SCREEN_WIDTH};
use super::super::super::config::{TOUCH_WIZARD_SESSION_EVENTS, TOUCH_WIZARD_SWIPE_TRACE_SAMPLES};
use super::super::super::types::{TouchWizardSessionEvent, TouchWizardSwipeTraceSample};
use super::draw::{
    continue_button_bounds, precision_menu_button_bounds, swipe_mark_button_bounds,
    test_return_bounds, test_toggle_bounds, ButtonBounds,
};
use super::swipe::{clamp_to_u16, trace_direction_code, trace_speed_code, SwipeCaseTraceInput};
use super::*;

impl TouchCalibrationWizard {
    pub(super) fn shows_swipe_debug(&self) -> bool {
        matches!(self.phase, WizardPhase::SwipeRight)
    }

    pub(super) fn shows_continue_button(&self) -> bool {
        matches!(self.phase, WizardPhase::SwipeRight | WizardPhase::Complete)
    }

    pub(super) fn continue_button_label(&self) -> &'static str {
        match self.phase {
            WizardPhase::SwipeRight => "SKIP CASE",
            WizardPhase::Complete => "EXIT",
            _ => "",
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

    pub(super) fn precision_menu_action(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Option<PrecisionMenuAction> {
        let (calibrate, test, continue_button) = precision_menu_button_bounds(width, height);
        if point_in_bounds(x, y, calibrate) {
            Some(PrecisionMenuAction::Calibrate)
        } else if point_in_bounds(x, y, test) {
            Some(PrecisionMenuAction::Test)
        } else if point_in_bounds(x, y, continue_button) {
            Some(PrecisionMenuAction::Continue)
        } else {
            None
        }
    }

    pub(super) fn test_toggle_hit(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        point_in_bounds(x, y, test_toggle_bounds(width, height))
    }

    pub(super) fn test_return_hit(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        point_in_bounds(x, y, test_return_bounds(width, height))
    }

    pub(super) fn open_precision_test(&mut self) {
        self.phase = WizardPhase::PrecisionTest;
        self.hint = "Tap anywhere to compare touch coordinates.";
        self.last_test_touch = None;
        self.test_mode = TestCoordinateMode::Calibrated;
    }

    pub(super) fn return_to_precision_menu(&mut self) {
        self.phase = WizardPhase::PrecisionMenu;
        self.hint = if self.calibration.is_some() {
            "Calibration ready. Test it or continue to swipes."
        } else {
            "Calibrate precision before testing swipes."
        };
        self.calibration_pending_return = false;
        self.last_test_touch = None;
    }

    pub(super) fn toggle_test_mode(&mut self) {
        self.test_mode = match self.test_mode {
            TestCoordinateMode::Calibrated => TestCoordinateMode::Uncalibrated,
            TestCoordinateMode::Uncalibrated => TestCoordinateMode::Calibrated,
        };
        self.hint = match self.test_mode {
            TestCoordinateMode::Calibrated => "Calibrated touches are circles.",
            TestCoordinateMode::Uncalibrated => "Uncalibrated touches are crosses.",
        };
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
        let prev_swipe_trace = self.swipe_trace;
        let prev_last_swipe = self.last_swipe;

        match self.phase {
            WizardPhase::PrecisionMenu => {
                self.enter_swipe_phase(t_ms, "Guided swipes ready.");
            }
            WizardPhase::Calibrate | WizardPhase::PrecisionTest => {}
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
            || self.swipe_trace != prev_swipe_trace
            || self.last_swipe != prev_last_swipe
    }

    pub(super) fn enter_swipe_phase(&mut self, t_ms: u64, hint: &'static str) {
        self.phase = WizardPhase::SwipeRight;
        self.hint = hint;
        self.calibration_pending_return = false;
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
            WizardPhase::PrecisionMenu => "Touch precision",
            WizardPhase::Calibrate => "Calibration",
            WizardPhase::PrecisionTest => "Precision test",
            WizardPhase::SwipeRight => "Guided Swipes",
            WizardPhase::Complete => "Done",
            WizardPhase::Closed => "",
        }
    }

    pub(super) fn primary_instruction(&self) -> &'static str {
        match self.phase {
            WizardPhase::PrecisionMenu => "Calibrate or test touch precision.",
            WizardPhase::Calibrate => "Touch all four corner dots.",
            WizardPhase::PrecisionTest => "Tap to show the selected coordinates.",
            WizardPhase::SwipeRight => "Perform the guided swipe case.",
            WizardPhase::Complete => "Touch test complete.",
            WizardPhase::Closed => "",
        }
    }

    pub(super) fn secondary_instruction(&self) -> &'static str {
        match self.phase {
            WizardPhase::PrecisionMenu => "Continue opens guided swipes.",
            WizardPhase::Calibrate => "Completed dots become solid.",
            WizardPhase::PrecisionTest => "Use the center toggle to switch modes.",
            WizardPhase::SwipeRight => {
                "FROM->TO + direction. Speed logged. Use I JUST SWIPED or SKIP CASE."
            }
            WizardPhase::Complete => "Exit with the EXIT button.",
            WizardPhase::Closed => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PrecisionMenuAction {
    Calibrate,
    Test,
    Continue,
}

fn point_in_bounds(x: i32, y: i32, bounds: ButtonBounds) -> bool {
    let (left, top, width, height) = bounds;
    x >= left && x < left + width && y >= top && y < top + height
}
