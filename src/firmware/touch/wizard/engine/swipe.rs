use super::super::super::types::{
    TouchEvent, TouchEventKind, TouchSwipeDirection, TouchWizardSessionEvent,
};
use super::*;

pub(super) struct SwipeCaseTraceInput {
    pub(super) t_ms: u64,
    pub(super) case_index: u8,
    pub(super) case: Option<SwipeCaseSpec>,
    pub(super) verdict: u8,
    pub(super) classified_direction: Option<TouchSwipeDirection>,
    pub(super) start: SwipePoint,
    pub(super) end: SwipePoint,
    pub(super) duration_ms: u16,
    pub(super) move_count: u16,
    pub(super) max_travel_px: u16,
    pub(super) release_debounce_ms: u16,
    pub(super) dropout_count: u16,
}

impl TouchCalibrationWizard {
    pub(super) fn clear_swipe_debug(&mut self) {
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

    pub(super) fn current_swipe_case(&self, width: i32, height: i32) -> Option<SwipeCaseSpec> {
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

    pub(super) fn advance_swipe_case_or_complete(
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

    pub(super) fn update_swipe_debug(&mut self, event: TouchEvent) {
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

    pub(super) fn on_swipe_trace_down(
        &mut self,
        start_x: i32,
        start_y: i32,
        x: i32,
        y: i32,
    ) -> bool {
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

    pub(super) fn on_swipe_trace_move(&mut self, x: i32, y: i32) -> bool {
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

    pub(super) fn append_swipe_trace_point(&mut self, x: i32, y: i32) {
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

pub(super) fn swipe_case_matches(
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

pub(super) fn swipe_start_matches(case: SwipeCaseSpec, start: SwipePoint) -> bool {
    squared_distance_i32(start.x, start.y, case.start.x, case.start.y)
        <= SWIPE_CASE_START_RADIUS_PX * SWIPE_CASE_START_RADIUS_PX
}

pub(super) fn squared_distance_i32(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    let dx = ax.saturating_sub(bx);
    let dy = ay.saturating_sub(by);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

pub(super) fn trace_direction_code(direction: TouchSwipeDirection) -> u8 {
    match direction {
        TouchSwipeDirection::Left => 0,
        TouchSwipeDirection::Right => 1,
        TouchSwipeDirection::Up => 2,
        TouchSwipeDirection::Down => 3,
    }
}

pub(super) fn trace_speed_code(speed: SwipeSpeedTier) -> u8 {
    match speed {
        SwipeSpeedTier::ExtraFast => 0,
        SwipeSpeedTier::Fast => 1,
        SwipeSpeedTier::Medium => 2,
        SwipeSpeedTier::Slow => 3,
    }
}

pub(super) fn clamp_to_u16(value: i32) -> u16 {
    value.clamp(0, u16::MAX as i32) as u16
}
