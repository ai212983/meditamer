use core::fmt::Write;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
};
use heapless::String;
use u8g2_fonts::types::{FontColor, HorizontalAlignment, VerticalPosition};

use crate::firmware::{
    config::{META_FONT, SCREEN_HEIGHT, SCREEN_WIDTH, TITLE_FONT},
    types::InkplateDriver,
};

use super::{
    config::{TOUCH_WIZARD_SESSION_EVENTS, TOUCH_WIZARD_SWIPE_TRACE_SAMPLES},
    types::{
        TouchEvent, TouchEventKind, TouchSwipeDirection, TouchWizardSessionEvent,
        TouchWizardSwipeTraceSample,
    },
};

const TARGET_RADIUS_PX: i32 = 26;
const TARGET_HIT_RADIUS_PX: i32 = TARGET_RADIUS_PX;
const SWIPE_TRACE_MAX_POINTS: usize = 32;
const CONTINUE_BUTTON_WIDTH: i32 = 192;
const CONTINUE_BUTTON_HEIGHT: i32 = 52;
const SWIPE_MARK_BUTTON_WIDTH: i32 = 232;
const SWIPE_MARK_BUTTON_HEIGHT: i32 = 44;
const SWIPE_CASE_COUNT: u8 = 8;
const SWIPE_CASE_START_RADIUS_PX: i32 = 60;
const SWIPE_CASE_END_RADIUS_PX: i32 = 72;
const SWIPE_CASE_REQUIRE_SPEED_MATCH: bool = false;
const TRACE_DIRECTION_UNKNOWN: u8 = 0xFF;
const TRACE_SPEED_UNKNOWN: u8 = 0xFF;
const TRACE_VERDICT_PASS: u8 = 0;
const TRACE_VERDICT_MISMATCH: u8 = 1;
const TRACE_VERDICT_RELEASE_NO_SWIPE: u8 = 2;
const TRACE_VERDICT_MANUAL_MARK: u8 = 3;
const TRACE_VERDICT_SKIP: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WizardPhase {
    Intro,
    TapCenter,
    TapTopLeft,
    TapBottomRight,
    SwipeRight,
    Complete,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WizardDispatch {
    Inactive,
    Consumed,
    Finished,
}

pub(crate) struct TouchCalibrationWizard {
    phase: WizardPhase,
    hint: &'static str,
    last_tap: Option<TapAttempt>,
    swipe_trace: SwipeTrace,
    last_swipe: Option<SwipeAttempt>,
    swipe_trace_pending_points: u8,
    swipe_debug: SwipeDebugStats,
    swipe_case_index: u8,
    swipe_case_passed: u8,
    swipe_case_failed: u16,
    swipe_case_attempts: u16,
    manual_swipe_marks: u16,
    pending_swipe_release: Option<PendingSwipeRelease>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TapAttempt {
    x: i32,
    y: i32,
    hit: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SwipePoint {
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwipeTrace {
    points: [SwipePoint; SWIPE_TRACE_MAX_POINTS],
    len: u8,
}

impl Default for SwipeTrace {
    fn default() -> Self {
        Self {
            points: [SwipePoint::default(); SWIPE_TRACE_MAX_POINTS],
            len: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwipeAttempt {
    start: SwipePoint,
    end: SwipePoint,
    accepted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingSwipeRelease {
    t_ms: u64,
    start: SwipePoint,
    end: SwipePoint,
    duration_ms: u16,
    move_count: u16,
    max_travel_px: u16,
    release_debounce_ms: u16,
    dropout_count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwipeSpeedTier {
    ExtraFast,
    Fast,
    Medium,
    Slow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwipeCaseSpec {
    direction: TouchSwipeDirection,
    speed: SwipeSpeedTier,
    start: SwipePoint,
    end: SwipePoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwipeDebugKind {
    None,
    Down,
    Move,
    Up,
    Swipe(TouchSwipeDirection),
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwipeDebugStats {
    down_count: u16,
    move_count: u16,
    up_count: u16,
    swipe_count: u16,
    cancel_count: u16,
    last_kind: SwipeDebugKind,
    last_start: SwipePoint,
    last_end: SwipePoint,
    last_duration_ms: u16,
    last_move_count: u16,
    last_max_travel_px: u16,
    last_release_debounce_ms: u16,
    last_dropout_count: u16,
}

impl Default for SwipeDebugStats {
    fn default() -> Self {
        Self {
            down_count: 0,
            move_count: 0,
            up_count: 0,
            swipe_count: 0,
            cancel_count: 0,
            last_kind: SwipeDebugKind::None,
            last_start: SwipePoint::default(),
            last_end: SwipePoint::default(),
            last_duration_ms: 0,
            last_move_count: 0,
            last_max_travel_px: 0,
            last_release_debounce_ms: 0,
            last_dropout_count: 0,
        }
    }
}

impl TouchCalibrationWizard {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            phase: if enabled {
                WizardPhase::Intro
            } else {
                WizardPhase::Closed
            },
            hint: "",
            last_tap: None,
            swipe_trace: SwipeTrace::default(),
            last_swipe: None,
            swipe_trace_pending_points: 0,
            swipe_debug: SwipeDebugStats::default(),
            swipe_case_index: 0,
            swipe_case_passed: 0,
            swipe_case_failed: 0,
            swipe_case_attempts: 0,
            manual_swipe_marks: 0,
            pending_swipe_release: None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !matches!(self.phase, WizardPhase::Closed)
    }

    pub(crate) async fn render_full(&self, display: &mut InkplateDriver) {
        self.render_with_refresh(display, true).await;
    }

    pub(crate) async fn render_partial(&self, display: &mut InkplateDriver) {
        self.render_with_refresh(display, false).await;
    }

    async fn render_with_refresh(&self, display: &mut InkplateDriver, full_refresh: bool) {
        if !self.is_active() {
            return;
        }

        let width = display.width() as i32;
        let height = display.height() as i32;
        let _ = display.clear(BinaryColor::Off);

        draw_frame(display, width, height);
        draw_centered_text(display, &TITLE_FONT, "TOUCH CALIBRATION WIZARD", 40);
        draw_centered_text(display, &META_FONT, self.step_progress_text(), 74);
        draw_centered_text(display, &META_FONT, self.primary_instruction(), 120);
        draw_centered_text(display, &META_FONT, self.secondary_instruction(), 154);
        if matches!(self.phase, WizardPhase::SwipeRight) {
            if let Some(case) = self.current_swipe_case(width, height) {
                draw_swipe_case_target(display, case);
                let mut case_line: String<96> = String::new();
                let _ = write!(
                    &mut case_line,
                    "Case {}/{}: {} {}",
                    self.swipe_case_index.saturating_add(1),
                    SWIPE_CASE_COUNT,
                    swipe_dir_label(case.direction),
                    swipe_speed_label(case.speed),
                );
                draw_centered_text(display, &META_FONT, &case_line, 182);
            }
        }

        if let Some((tx, ty)) = self.target_point(width, height) {
            draw_target(display, tx, ty);
            if let Some(last_tap) = self.last_tap {
                draw_tap_attempt_feedback(display, tx, ty, last_tap);
            }
        }
        if self.shows_swipe_debug() {
            draw_swipe_debug(
                display,
                self.swipe_trace,
                self.last_swipe,
                self.swipe_debug,
                self.swipe_case_passed,
                self.swipe_case_attempts,
                self.manual_swipe_marks,
            );
        }
        if self.shows_continue_button() {
            draw_continue_button(display, width, height, self.continue_button_label());
        }
        if self.shows_swipe_mark_button() {
            draw_swipe_mark_button(display, width, height);
        }

        let footer = if self.hint.is_empty() {
            "Follow the target and gesture prompts."
        } else {
            self.hint
        };
        draw_centered_text(display, &META_FONT, footer, height - 42);

        if full_refresh {
            let _ = display.display_bw_async(false).await;
        } else {
            let _ = display.display_bw_partial_async(false).await;
        }
    }
}

impl TouchCalibrationWizard {
    pub(crate) async fn handle_event(
        &mut self,
        display: &mut InkplateDriver,
        event: TouchEvent,
    ) -> WizardDispatch {
        if !self.is_active() {
            return WizardDispatch::Inactive;
        }

        let width = display.width() as i32;
        let height = display.height() as i32;
        let mut changed = false;

        let is_action_tap = matches!(event.kind, TouchEventKind::Tap | TouchEventKind::LongPress);
        let continue_hit = is_action_tap
            && self.shows_continue_button()
            && self.continue_button_hit(event.x as i32, event.y as i32, width, height);
        let swipe_mark_hit = is_action_tap
            && self.shows_swipe_mark_button()
            && self.swipe_mark_button_hit(event.x as i32, event.y as i32, width, height);
        if self.resolve_pending_swipe_release(event, continue_hit, swipe_mark_hit) {
            changed = true;
        }

        if swipe_mark_hit {
            // Handle manual swipe markers before consuming current tap in debug
            // counters so we can associate marker with the preceding gesture.
            changed = self.on_manual_swipe_mark(event.t_ms);
        } else if continue_hit {
            changed = self.on_continue_button(event.t_ms);
        } else {
            self.update_swipe_debug(event);
            match event.kind {
                TouchEventKind::Down => {
                    // Handle tap-target steps on Down for more immediate and reliable feedback.
                    if self.is_tap_step() || matches!(self.phase, WizardPhase::Intro) {
                        changed = self.on_tap(event.t_ms, event.x, event.y, width, height);
                    } else if matches!(self.phase, WizardPhase::SwipeRight) {
                        let is_ui_touch =
                            self.continue_button_hit(event.x as i32, event.y as i32, width, height)
                                || self.swipe_mark_button_hit(
                                    event.x as i32,
                                    event.y as i32,
                                    width,
                                    height,
                                );
                        if !is_ui_touch {
                            changed = self.on_swipe_trace_down(
                                event.start_x as i32,
                                event.start_y as i32,
                                event.x as i32,
                                event.y as i32,
                            );
                        }
                    }
                }
                TouchEventKind::Tap => {
                    // Keep Tap as Intro fallback, but avoid double-processing tap-step touches
                    // that were already handled on Down.
                    if matches!(self.phase, WizardPhase::Intro) {
                        changed = self.on_tap(event.t_ms, event.x, event.y, width, height);
                    }
                }
                TouchEventKind::Up => {
                    if matches!(self.phase, WizardPhase::SwipeRight) {
                        changed = self.on_swipe_release(event) || changed;
                    } else if matches!(self.phase, WizardPhase::Intro) {
                        changed = self.on_tap(event.t_ms, event.x, event.y, width, height);
                    }
                }
                TouchEventKind::Move => {
                    if matches!(self.phase, WizardPhase::SwipeRight) {
                        changed = self.on_swipe_trace_move(event.x as i32, event.y as i32);
                    }
                }
                TouchEventKind::LongPress => {
                    // Fallback for panels where Tap classification is timing-sensitive.
                    if matches!(self.phase, WizardPhase::Intro) {
                        changed = self.on_tap(event.t_ms, event.x, event.y, width, height);
                    }
                }
                TouchEventKind::Swipe(direction) => {
                    changed = self.on_swipe_event(event, direction);
                }
                TouchEventKind::Cancel => {
                    self.hint = "Touch canceled. Retry current step.";
                    self.last_tap = None;
                    changed = true;
                }
            }
        }

        let finished = matches!(self.phase, WizardPhase::Closed);
        if finished {
            return WizardDispatch::Finished;
        }

        if changed {
            self.render_partial(display).await;
        }
        WizardDispatch::Consumed
    }

    fn on_tap(&mut self, t_ms: u64, x: u16, y: u16, width: i32, height: i32) -> bool {
        let px = x as i32;
        let py = y as i32;
        let prev_phase = self.phase;
        let prev_hint = self.hint;
        let prev_last_tap = self.last_tap;

        match self.phase {
            WizardPhase::Intro => {
                self.phase = WizardPhase::TapCenter;
                self.hint = "Step 1 started. Tap center target.";
                self.last_tap = None;
            }
            WizardPhase::TapCenter => {
                let hit = self.tap_hits_target(px, py, width, height);
                if hit {
                    self.phase = WizardPhase::TapTopLeft;
                    self.hint = "Center accepted.";
                    self.last_tap = None;
                } else {
                    self.hint = "Missed center target. See marker.";
                    self.last_tap = Some(TapAttempt { x: px, y: py, hit });
                }
            }
            WizardPhase::TapTopLeft => {
                let hit = self.tap_hits_target(px, py, width, height);
                if hit {
                    self.phase = WizardPhase::TapBottomRight;
                    self.hint = "Top-left accepted.";
                    self.last_tap = None;
                } else {
                    self.hint = "Missed top-left target. See marker.";
                    self.last_tap = Some(TapAttempt { x: px, y: py, hit });
                }
            }
            WizardPhase::TapBottomRight => {
                let hit = self.tap_hits_target(px, py, width, height);
                if hit {
                    self.enter_swipe_phase(t_ms, "Tap targets complete. Guided swipes start.");
                } else {
                    self.hint = "Missed bottom-right target. See marker.";
                    self.last_tap = Some(TapAttempt { x: px, y: py, hit });
                }
            }
            WizardPhase::SwipeRight => {
                self.hint = "Do current guided swipe case.";
                self.last_tap = None;
            }
            WizardPhase::Complete => {
                self.hint = "Press CONTINUE to exit.";
                self.last_tap = None;
            }
            WizardPhase::Closed => {}
        }

        self.phase != prev_phase || self.hint != prev_hint || self.last_tap != prev_last_tap
    }

    fn on_swipe_event(&mut self, event: TouchEvent, direction: TouchSwipeDirection) -> bool {
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

    fn on_swipe_release(&mut self, event: TouchEvent) -> bool {
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

    fn resolve_pending_swipe_release(
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

    fn is_tap_step(&self) -> bool {
        matches!(
            self.phase,
            WizardPhase::TapCenter | WizardPhase::TapTopLeft | WizardPhase::TapBottomRight
        )
    }
}

fn pending_release_matches_swipe(pending: PendingSwipeRelease, event: TouchEvent) -> bool {
    matches!(event.kind, TouchEventKind::Swipe(_))
        && event.t_ms == pending.t_ms
        && event.start_x as i32 == pending.start.x
        && event.start_y as i32 == pending.start.y
        && event.duration_ms == pending.duration_ms
        && event.move_count == pending.move_count
        && event.max_travel_px == pending.max_travel_px
        && event.release_debounce_ms == pending.release_debounce_ms
        && event.dropout_count == pending.dropout_count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending_release() -> PendingSwipeRelease {
        PendingSwipeRelease {
            t_ms: 120,
            start: SwipePoint { x: 100, y: 200 },
            end: SwipePoint { x: 140, y: 202 },
            duration_ms: 120,
            move_count: 3,
            max_travel_px: 72,
            release_debounce_ms: 56,
            dropout_count: 1,
        }
    }

    #[test]
    fn pending_release_matches_same_swipe_even_if_end_differs() {
        let pending = pending_release();
        let swipe = TouchEvent {
            kind: TouchEventKind::Swipe(TouchSwipeDirection::Right),
            t_ms: pending.t_ms,
            x: 220,
            y: 206,
            start_x: pending.start.x as u16,
            start_y: pending.start.y as u16,
            duration_ms: pending.duration_ms,
            touch_count: 0,
            move_count: pending.move_count,
            max_travel_px: pending.max_travel_px,
            release_debounce_ms: pending.release_debounce_ms,
            dropout_count: pending.dropout_count,
        };

        assert!(pending_release_matches_swipe(pending, swipe));
    }

    #[test]
    fn pending_release_rejects_unrelated_swipe() {
        let pending = pending_release();
        let swipe = TouchEvent {
            kind: TouchEventKind::Swipe(TouchSwipeDirection::Right),
            t_ms: pending.t_ms + 1,
            x: 220,
            y: 206,
            start_x: pending.start.x as u16,
            start_y: pending.start.y as u16,
            duration_ms: pending.duration_ms,
            touch_count: 0,
            move_count: pending.move_count,
            max_travel_px: pending.max_travel_px,
            release_debounce_ms: pending.release_debounce_ms,
            dropout_count: pending.dropout_count,
        };

        assert!(!pending_release_matches_swipe(pending, swipe));
    }
}

#[derive(Clone, Copy)]
struct SwipeCaseTraceInput {
    t_ms: u64,
    case_index: u8,
    case: Option<SwipeCaseSpec>,
    verdict: u8,
    classified_direction: Option<TouchSwipeDirection>,
    start: SwipePoint,
    end: SwipePoint,
    duration_ms: u16,
    move_count: u16,
    max_travel_px: u16,
    release_debounce_ms: u16,
    dropout_count: u16,
}

impl TouchCalibrationWizard {
    fn clear_swipe_debug(&mut self) {
        self.swipe_trace = SwipeTrace::default();
        self.last_swipe = None;
        self.swipe_trace_pending_points = 0;
        self.swipe_debug = SwipeDebugStats::default();
        self.swipe_case_index = 0;
        self.swipe_case_passed = 0;
        self.swipe_case_failed = 0;
        self.swipe_case_attempts = 0;
        self.manual_swipe_marks = 0;
        self.pending_swipe_release = None;
    }

    fn current_swipe_case(&self, width: i32, height: i32) -> Option<SwipeCaseSpec> {
        let idx = self.swipe_case_index;
        if idx >= SWIPE_CASE_COUNT {
            return None;
        }

        let speed = match idx % 4 {
            0 => SwipeSpeedTier::ExtraFast,
            1 => SwipeSpeedTier::Fast,
            2 => SwipeSpeedTier::Medium,
            _ => SwipeSpeedTier::Slow,
        };
        let lane = (idx % 4) as i32;

        let right_start_x = width / 6;
        let right_end_x = width * 5 / 6;
        let right_y = height / 3 + 24 + lane * 34;

        let down_x = width / 2 - 54 + lane * 36;
        let down_start_y = height / 3 - 16;
        let down_end_y = height * 3 / 4;

        if idx < 4 {
            Some(SwipeCaseSpec {
                direction: TouchSwipeDirection::Right,
                speed,
                start: SwipePoint {
                    x: right_start_x,
                    y: right_y,
                },
                end: SwipePoint {
                    x: right_end_x,
                    y: right_y,
                },
            })
        } else {
            Some(SwipeCaseSpec {
                direction: TouchSwipeDirection::Down,
                speed,
                start: SwipePoint {
                    x: down_x,
                    y: down_start_y,
                },
                end: SwipePoint {
                    x: down_x,
                    y: down_end_y,
                },
            })
        }
    }

    fn advance_swipe_case_or_complete(
        &mut self,
        t_ms: u64,
        next_hint: &'static str,
        done_hint: &'static str,
    ) {
        if self.swipe_case_index.saturating_add(1) < SWIPE_CASE_COUNT {
            self.swipe_case_index = self.swipe_case_index.saturating_add(1);
            self.hint = next_hint;
            self.swipe_trace = SwipeTrace::default();
            self.swipe_trace_pending_points = 0;
        } else {
            self.phase = WizardPhase::Complete;
            self.hint = done_hint;
            self.emit_swipe_session_event(TouchWizardSessionEvent::End { t_ms });
        }
    }

    fn update_swipe_debug(&mut self, event: TouchEvent) {
        if !self.shows_swipe_debug() {
            return;
        }

        self.swipe_debug.last_start = SwipePoint {
            x: event.start_x as i32,
            y: event.start_y as i32,
        };
        self.swipe_debug.last_end = SwipePoint {
            x: event.x as i32,
            y: event.y as i32,
        };
        self.swipe_debug.last_duration_ms = event.duration_ms;
        self.swipe_debug.last_move_count = event.move_count;
        self.swipe_debug.last_max_travel_px = event.max_travel_px;
        self.swipe_debug.last_release_debounce_ms = event.release_debounce_ms;
        self.swipe_debug.last_dropout_count = event.dropout_count;

        match event.kind {
            TouchEventKind::Down => {
                self.swipe_debug.down_count = self.swipe_debug.down_count.saturating_add(1);
                self.swipe_debug.last_kind = SwipeDebugKind::Down;
            }
            TouchEventKind::Move => {
                self.swipe_debug.move_count = self.swipe_debug.move_count.saturating_add(1);
                self.swipe_debug.last_kind = SwipeDebugKind::Move;
            }
            TouchEventKind::Up => {
                self.swipe_debug.up_count = self.swipe_debug.up_count.saturating_add(1);
                self.swipe_debug.last_kind = SwipeDebugKind::Up;
            }
            TouchEventKind::Swipe(direction) => {
                self.swipe_debug.swipe_count = self.swipe_debug.swipe_count.saturating_add(1);
                self.swipe_debug.last_kind = SwipeDebugKind::Swipe(direction);
            }
            TouchEventKind::Cancel => {
                self.swipe_debug.cancel_count = self.swipe_debug.cancel_count.saturating_add(1);
                self.swipe_debug.last_kind = SwipeDebugKind::Cancel;
            }
            TouchEventKind::Tap | TouchEventKind::LongPress => {}
        }
    }

    fn on_swipe_trace_down(&mut self, start_x: i32, start_y: i32, x: i32, y: i32) -> bool {
        self.swipe_trace = SwipeTrace::default();
        self.swipe_trace.points[0] = SwipePoint {
            x: start_x,
            y: start_y,
        };
        self.swipe_trace.len = 1;
        if squared_distance_i32(start_x, start_y, x, y) >= 9 {
            self.append_swipe_trace_point(x, y);
        }
        self.swipe_trace_pending_points = 0;
        // Avoid full wizard redraw on touch down; display task already renders
        // lightweight touch dots, and blocking redraws here can starve swipe sampling.
        false
    }

    fn on_swipe_trace_move(&mut self, x: i32, y: i32) -> bool {
        if self.swipe_trace.len == 0 {
            return self.on_swipe_trace_down(x, y, x, y);
        }
        let prev_len = self.swipe_trace.len;
        self.append_swipe_trace_point(x, y);
        if self.swipe_trace.len == prev_len {
            return false;
        }
        self.swipe_trace_pending_points = self.swipe_trace_pending_points.saturating_add(1);
        // Defer redraw until Up/Swipe event to keep gesture sampling responsive.
        false
    }

    fn append_swipe_trace_point(&mut self, x: i32, y: i32) {
        if self.swipe_trace.len == 0 {
            self.swipe_trace.points[0] = SwipePoint { x, y };
            self.swipe_trace.len = 1;
            return;
        }

        let last_idx = self.swipe_trace.len.saturating_sub(1) as usize;
        let last = self.swipe_trace.points[last_idx];
        if squared_distance_i32(x, y, last.x, last.y) < 9 {
            return;
        }

        if (self.swipe_trace.len as usize) < SWIPE_TRACE_MAX_POINTS {
            self.swipe_trace.points[self.swipe_trace.len as usize] = SwipePoint { x, y };
            self.swipe_trace.len = self.swipe_trace.len.saturating_add(1);
        } else {
            let mut idx = 1usize;
            while idx < SWIPE_TRACE_MAX_POINTS {
                self.swipe_trace.points[idx - 1] = self.swipe_trace.points[idx];
                idx += 1;
            }
            self.swipe_trace.points[SWIPE_TRACE_MAX_POINTS - 1] = SwipePoint { x, y };
        }
    }

    fn shows_swipe_debug(&self) -> bool {
        matches!(self.phase, WizardPhase::SwipeRight | WizardPhase::Complete)
    }

    fn shows_continue_button(&self) -> bool {
        !matches!(self.phase, WizardPhase::Closed)
    }

    fn continue_button_label(&self) -> &'static str {
        match self.phase {
            WizardPhase::SwipeRight => "SKIP CASE",
            WizardPhase::Complete => "EXIT",
            _ => "CONTINUE",
        }
    }

    fn shows_swipe_mark_button(&self) -> bool {
        matches!(self.phase, WizardPhase::SwipeRight)
    }

    fn continue_button_hit(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        let (left, top, w, h) = continue_button_bounds(width, height);
        x >= left && x < left + w && y >= top && y < top + h
    }

    fn swipe_mark_button_hit(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        let (left, top, w, h) = swipe_mark_button_bounds(width, height);
        x >= left && x < left + w && y >= top && y < top + h
    }

    fn on_manual_swipe_mark(&mut self, t_ms: u64) -> bool {
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

    fn on_continue_button(&mut self, t_ms: u64) -> bool {
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

    fn tap_hits_target(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        let Some((tx, ty)) = self.target_point(width, height) else {
            return false;
        };
        squared_distance_i32(x, y, tx, ty) <= TARGET_HIT_RADIUS_PX * TARGET_HIT_RADIUS_PX
    }

    fn enter_swipe_phase(&mut self, t_ms: u64, hint: &'static str) {
        self.phase = WizardPhase::SwipeRight;
        self.hint = hint;
        self.last_tap = None;
        self.clear_swipe_debug();
        self.emit_swipe_session_event(TouchWizardSessionEvent::Start { t_ms });
    }

    fn emit_swipe_session_event(&self, event: TouchWizardSessionEvent) {
        let _ = TOUCH_WIZARD_SESSION_EVENTS.try_send(event);
    }

    fn emit_swipe_case_trace(&self, trace: SwipeCaseTraceInput) {
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

    fn step_progress_text(&self) -> &'static str {
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

    fn primary_instruction(&self) -> &'static str {
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

    fn secondary_instruction(&self) -> &'static str {
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

    fn target_point(&self, width: i32, height: i32) -> Option<(i32, i32)> {
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

pub(crate) async fn render_touch_wizard_waiting_screen(display: &mut InkplateDriver) {
    let width = display.width() as i32;
    let height = display.height() as i32;
    let _ = display.clear(BinaryColor::Off);

    draw_frame(display, width, height);
    draw_centered_text(display, &TITLE_FONT, "TOUCH CALIBRATION WIZARD", 40);
    draw_centered_text(display, &META_FONT, "Waiting For Touch Controller", 86);
    draw_centered_text(
        display,
        &META_FONT,
        "Touch init failed or disconnected.",
        126,
    );
    draw_centered_text(
        display,
        &META_FONT,
        "Keep device powered and wait for retry.",
        158,
    );
    draw_centered_text(
        display,
        &META_FONT,
        "Wizard will start automatically.",
        height - 42,
    );

    let _ = display.display_bw_async(false).await;
}

fn draw_frame(display: &mut InkplateDriver, width: i32, height: i32) {
    let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let _ = Rectangle::new(
        Point::new(12, 12),
        Size::new((width - 24).max(1) as u32, (height - 24).max(1) as u32),
    )
    .into_styled(style)
    .draw(display);
}

fn draw_centered_text(
    display: &mut InkplateDriver,
    renderer: &u8g2_fonts::FontRenderer,
    text: &str,
    center_y: i32,
) {
    let _ = renderer.render_aligned(
        text,
        Point::new(SCREEN_WIDTH / 2, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
}

fn draw_left_text(
    display: &mut InkplateDriver,
    renderer: &u8g2_fonts::FontRenderer,
    text: &str,
    left_x: i32,
    center_y: i32,
) {
    let _ = renderer.render_aligned(
        text,
        Point::new(left_x, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Left,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
}

fn draw_target(display: &mut InkplateDriver, x: i32, y: i32) {
    let style = PrimitiveStyle::with_stroke(BinaryColor::On, 2);
    let _ = Circle::new(
        Point::new(x - TARGET_RADIUS_PX, y - TARGET_RADIUS_PX),
        (TARGET_RADIUS_PX * 2).max(1) as u32,
    )
    .into_styled(style)
    .draw(display);

    let _ = Line::new(Point::new(x - 10, y), Point::new(x + 10, y))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);
    let _ = Line::new(Point::new(x, y - 10), Point::new(x, y + 10))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);
}

fn draw_swipe_case_target(display: &mut InkplateDriver, case: SwipeCaseSpec) {
    let _ = Line::new(
        Point::new(case.start.x, case.start.y),
        Point::new(case.end.x, case.end.y),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
    .draw(display);

    let _ = Circle::new(
        Point::new(
            case.start.x - SWIPE_CASE_START_RADIUS_PX,
            case.start.y - SWIPE_CASE_START_RADIUS_PX,
        ),
        (SWIPE_CASE_START_RADIUS_PX * 2) as u32,
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);

    let _ = Circle::new(
        Point::new(
            case.end.x - SWIPE_CASE_END_RADIUS_PX,
            case.end.y - SWIPE_CASE_END_RADIUS_PX,
        ),
        (SWIPE_CASE_END_RADIUS_PX * 2) as u32,
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);

    let vx = case.end.x.saturating_sub(case.start.x);
    let vy = case.end.y.saturating_sub(case.start.y);
    let vmax = vx.abs().max(vy.abs()).max(1);
    let ux = vx.saturating_mul(16) / vmax;
    let uy = vy.saturating_mul(16) / vmax;
    let px = -uy / 2;
    let py = ux / 2;
    let ax = case.end.x.saturating_sub(ux);
    let ay = case.end.y.saturating_sub(uy);

    let _ = Line::new(
        Point::new(ax.saturating_add(px), ay.saturating_add(py)),
        Point::new(case.end.x, case.end.y),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);
    let _ = Line::new(
        Point::new(ax.saturating_sub(px), ay.saturating_sub(py)),
        Point::new(case.end.x, case.end.y),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);

    draw_left_text(
        display,
        &META_FONT,
        "FROM",
        case.start.x.saturating_sub(34),
        case.start.y.saturating_sub(SWIPE_CASE_START_RADIUS_PX + 12),
    );
    draw_left_text(
        display,
        &META_FONT,
        "TO",
        case.end.x.saturating_sub(14),
        case.end.y.saturating_sub(SWIPE_CASE_END_RADIUS_PX + 12),
    );
}

fn swipe_speed_label(speed: SwipeSpeedTier) -> &'static str {
    match speed {
        SwipeSpeedTier::ExtraFast => "extrafast",
        SwipeSpeedTier::Fast => "fast",
        SwipeSpeedTier::Medium => "medium",
        SwipeSpeedTier::Slow => "slow",
    }
}

fn swipe_dir_label(direction: TouchSwipeDirection) -> &'static str {
    match direction {
        TouchSwipeDirection::Left => "left",
        TouchSwipeDirection::Right => "right",
        TouchSwipeDirection::Up => "up",
        TouchSwipeDirection::Down => "down",
    }
}

fn draw_swipe_debug(
    display: &mut InkplateDriver,
    trace: SwipeTrace,
    attempt: Option<SwipeAttempt>,
    stats: SwipeDebugStats,
    case_passed: u8,
    case_attempts: u16,
    manual_marks: u16,
) {
    if trace.len >= 2 {
        let mut idx = 1usize;
        while idx < trace.len as usize {
            let a = trace.points[idx - 1];
            let b = trace.points[idx];
            let _ = Line::new(Point::new(a.x, a.y), Point::new(b.x, b.y))
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                .draw(display);
            idx += 1;
        }
    } else if trace.len == 1 {
        let p = trace.points[0];
        let _ = Circle::new(Point::new(p.x - 3, p.y - 3), 6)
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display);
    }

    if let Some(attempt) = attempt {
        let _ = Line::new(
            Point::new(attempt.start.x, attempt.start.y),
            Point::new(attempt.end.x, attempt.end.y),
        )
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);

        let _ = Circle::new(Point::new(attempt.start.x - 4, attempt.start.y - 4), 8)
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display);
        if attempt.accepted {
            let _ = Circle::new(Point::new(attempt.end.x - 5, attempt.end.y - 5), 10)
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                .draw(display);
        } else {
            let _ = Line::new(
                Point::new(attempt.end.x - 7, attempt.end.y - 7),
                Point::new(attempt.end.x + 7, attempt.end.y + 7),
            )
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display);
            let _ = Line::new(
                Point::new(attempt.end.x - 7, attempt.end.y + 7),
                Point::new(attempt.end.x + 7, attempt.end.y - 7),
            )
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display);
        }
    }

    let mut counts_line: String<64> = String::new();
    let _ = write!(
        &mut counts_line,
        "D/M/U/S/C: {}/{}/{}/{}/{}",
        stats.down_count, stats.move_count, stats.up_count, stats.swipe_count, stats.cancel_count
    );
    draw_left_text(display, &META_FONT, &counts_line, 32, 404);

    let mut case_line: String<64> = String::new();
    let _ = write!(
        &mut case_line,
        "cases pass/attempt: {}/{} marks={}",
        case_passed, case_attempts, manual_marks
    );
    draw_left_text(display, &META_FONT, &case_line, 32, 430);

    let last_kind = match stats.last_kind {
        SwipeDebugKind::None => "none",
        SwipeDebugKind::Down => "down",
        SwipeDebugKind::Move => "move",
        SwipeDebugKind::Up => "up",
        SwipeDebugKind::Swipe(TouchSwipeDirection::Left) => "swipe_left",
        SwipeDebugKind::Swipe(TouchSwipeDirection::Right) => "swipe_right",
        SwipeDebugKind::Swipe(TouchSwipeDirection::Up) => "swipe_up",
        SwipeDebugKind::Swipe(TouchSwipeDirection::Down) => "swipe_down",
        SwipeDebugKind::Cancel => "cancel",
    };
    let dx = stats.last_end.x.saturating_sub(stats.last_start.x);
    let dy = stats.last_end.y.saturating_sub(stats.last_start.y);
    let mut vector_line: String<96> = String::new();
    let _ = write!(
        &mut vector_line,
        "last={} dur={}ms dx={} dy={}",
        last_kind, stats.last_duration_ms, dx, dy
    );
    draw_left_text(display, &META_FONT, &vector_line, 32, 456);

    let (from, to) = if let Some(attempt) = attempt {
        (attempt.start, attempt.end)
    } else {
        (stats.last_start, stats.last_end)
    };
    let mut points_line: String<96> = String::new();
    let _ = write!(
        &mut points_line,
        "from=({}, {}) to=({}, {})",
        from.x, from.y, to.x, to.y
    );
    draw_left_text(display, &META_FONT, &points_line, 32, 476);
}

fn draw_continue_button(display: &mut InkplateDriver, width: i32, height: i32, label: &str) {
    let (left, top, w, h) = continue_button_bounds(width, height);
    let _ = Rectangle::new(
        Point::new(left, top),
        Size::new(w.max(1) as u32, h.max(1) as u32),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);
    draw_centered_text(display, &META_FONT, label, top + h / 2);
}

fn draw_swipe_mark_button(display: &mut InkplateDriver, width: i32, height: i32) {
    let (left, top, w, h) = swipe_mark_button_bounds(width, height);
    let _ = Rectangle::new(
        Point::new(left, top),
        Size::new(w.max(1) as u32, h.max(1) as u32),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);
    draw_centered_text(display, &META_FONT, "I JUST SWIPED", top + h / 2);
}

fn continue_button_bounds(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let w = CONTINUE_BUTTON_WIDTH.min(width - 24).max(80);
    let h = CONTINUE_BUTTON_HEIGHT;
    let left = (width - w) / 2;
    let top = height - 108;
    (left, top, w, h)
}

fn swipe_mark_button_bounds(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let w = SWIPE_MARK_BUTTON_WIDTH.min(width - 24).max(100);
    let h = SWIPE_MARK_BUTTON_HEIGHT;
    let left = (width - w) / 2;
    let top = height - 166;
    (left, top, w, h)
}

fn draw_tap_attempt_feedback(
    display: &mut InkplateDriver,
    target_x: i32,
    target_y: i32,
    tap: TapAttempt,
) {
    let _ = Line::new(Point::new(target_x, target_y), Point::new(tap.x, tap.y))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);

    if tap.hit {
        let _ = Circle::new(Point::new(tap.x - 5, tap.y - 5), 10)
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display);
    } else {
        let _ = Line::new(
            Point::new(tap.x - 7, tap.y - 7),
            Point::new(tap.x + 7, tap.y + 7),
        )
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);
        let _ = Line::new(
            Point::new(tap.x - 7, tap.y + 7),
            Point::new(tap.x + 7, tap.y - 7),
        )
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);
    }
}

fn swipe_speed_from_duration(duration_ms: u16) -> SwipeSpeedTier {
    if duration_ms <= 260 {
        SwipeSpeedTier::ExtraFast
    } else if duration_ms <= 520 {
        SwipeSpeedTier::Fast
    } else if duration_ms <= 900 {
        SwipeSpeedTier::Medium
    } else {
        SwipeSpeedTier::Slow
    }
}

fn swipe_case_matches(
    case: SwipeCaseSpec,
    start: SwipePoint,
    end: SwipePoint,
    duration_ms: u16,
    direction: Option<TouchSwipeDirection>,
    classified_as_swipe: bool,
) -> bool {
    let start_ok = swipe_start_matches(case, start);
    let end_ok = squared_distance_i32(end.x, end.y, case.end.x, case.end.y)
        <= SWIPE_CASE_END_RADIUS_PX * SWIPE_CASE_END_RADIUS_PX;
    let direction_ok = direction == Some(case.direction);
    let speed_ok =
        !SWIPE_CASE_REQUIRE_SPEED_MATCH || swipe_speed_from_duration(duration_ms) == case.speed;
    classified_as_swipe && start_ok && end_ok && direction_ok && speed_ok
}

fn swipe_start_matches(case: SwipeCaseSpec, start: SwipePoint) -> bool {
    squared_distance_i32(start.x, start.y, case.start.x, case.start.y)
        <= SWIPE_CASE_START_RADIUS_PX * SWIPE_CASE_START_RADIUS_PX
}

fn squared_distance_i32(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    let dx = ax.saturating_sub(bx);
    let dy = ay.saturating_sub(by);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

fn trace_direction_code(direction: TouchSwipeDirection) -> u8 {
    match direction {
        TouchSwipeDirection::Left => 0,
        TouchSwipeDirection::Right => 1,
        TouchSwipeDirection::Up => 2,
        TouchSwipeDirection::Down => 3,
    }
}

fn trace_speed_code(speed: SwipeSpeedTier) -> u8 {
    match speed {
        SwipeSpeedTier::ExtraFast => 0,
        SwipeSpeedTier::Fast => 1,
        SwipeSpeedTier::Medium => 2,
        SwipeSpeedTier::Slow => 3,
    }
}

fn clamp_to_u16(value: i32) -> u16 {
    value.clamp(0, u16::MAX as i32) as u16
}
