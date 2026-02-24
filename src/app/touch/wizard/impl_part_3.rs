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
            WizardPhase::SwipeRight => "FROM->TO + speed. Use I JUST SWIPED or SKIP CASE.",
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
