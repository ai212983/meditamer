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
