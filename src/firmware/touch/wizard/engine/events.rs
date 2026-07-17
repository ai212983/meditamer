use super::super::super::super::types::InkplateDriver;
use super::super::super::types::{TouchEvent, TouchEventKind};
use super::flow::PrecisionMenuAction;
use super::*;

impl TouchCalibrationWizard {
    pub(crate) async fn handle_event(
        &mut self,
        display: &mut InkplateDriver,
        raw_event: TouchEvent,
    ) -> WizardDispatch {
        if !self.is_active() {
            return WizardDispatch::Inactive;
        }

        let width = display.width() as i32;
        let height = display.height() as i32;
        let event = if matches!(self.phase, WizardPhase::Calibrate) {
            raw_event
        } else {
            self.calibrated_event(raw_event, width, height)
        };
        let phase_before = self.phase;
        let mut changed = false;

        let is_action_tap = matches!(event.kind, TouchEventKind::Tap | TouchEventKind::LongPress);
        let continue_hit = is_action_tap
            && self.shows_continue_button()
            && self.continue_button_hit(event.x as i32, event.y as i32, width, height);
        let swipe_mark_hit = is_action_tap
            && self.shows_swipe_mark_button()
            && self.swipe_mark_button_hit(event.x as i32, event.y as i32, width, height);
        if matches!(self.phase, WizardPhase::SwipeRight)
            && self.resolve_pending_swipe_release(event, continue_hit, swipe_mark_hit)
        {
            changed = true;
        }

        match self.phase {
            WizardPhase::PrecisionMenu => {
                if is_action_tap {
                    match self.precision_menu_action(event.x as i32, event.y as i32, width, height)
                    {
                        Some(PrecisionMenuAction::Calibrate) => {
                            self.start_calibration();
                            changed = true;
                        }
                        Some(PrecisionMenuAction::Test) => {
                            self.open_precision_test();
                            changed = true;
                        }
                        Some(PrecisionMenuAction::Continue) => {
                            changed = self.on_continue_button(event.t_ms);
                        }
                        None => {}
                    }
                }
            }
            WizardPhase::Calibrate => match raw_event.kind {
                TouchEventKind::Down if !self.calibration_pending_return => {
                    changed = self.record_calibration_touch(
                        raw_event.contact_x as i32,
                        raw_event.contact_y as i32,
                        width,
                        height,
                    );
                }
                TouchEventKind::Up if self.calibration_pending_return => {
                    self.return_to_precision_menu();
                    changed = true;
                }
                TouchEventKind::Cancel => {
                    if self.calibration_pending_return {
                        self.return_to_precision_menu();
                    } else {
                        self.hint = "Touch canceled. Touch the remaining dots.";
                    }
                    changed = true;
                }
                _ => {}
            },
            WizardPhase::PrecisionTest => {
                if is_action_tap
                    && self.test_return_hit(event.x as i32, event.y as i32, width, height)
                {
                    self.return_to_precision_menu();
                    changed = true;
                } else if is_action_tap
                    && self.test_toggle_hit(event.x as i32, event.y as i32, width, height)
                {
                    self.toggle_test_mode();
                    changed = true;
                } else if matches!(event.kind, TouchEventKind::Down)
                    && !self.test_toggle_hit(event.x as i32, event.y as i32, width, height)
                    && !self.test_return_hit(event.x as i32, event.y as i32, width, height)
                {
                    self.last_test_touch = Some(TestTouch {
                        raw: SwipePoint {
                            x: raw_event.contact_x as i32,
                            y: raw_event.contact_y as i32,
                        },
                        calibrated: SwipePoint {
                            x: event.contact_x as i32,
                            y: event.contact_y as i32,
                        },
                    });
                    changed = true;
                }
            }
            WizardPhase::SwipeRight => {
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
                            let is_ui_touch = self.continue_button_hit(
                                event.x as i32,
                                event.y as i32,
                                width,
                                height,
                            ) || self.swipe_mark_button_hit(
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
                        TouchEventKind::Tap | TouchEventKind::LongPress => {}
                        TouchEventKind::Up => {
                            changed = self.on_swipe_release(event) || changed;
                        }
                        TouchEventKind::Move => {
                            changed = self.on_swipe_trace_move(event.x as i32, event.y as i32);
                        }
                        TouchEventKind::Swipe(direction) => {
                            changed = self.on_swipe_event(event, direction);
                        }
                        TouchEventKind::Cancel => {
                            self.hint = "Touch canceled. Retry current step.";
                            changed = true;
                        }
                    }
                }
            }
            WizardPhase::Complete => {
                if continue_hit {
                    changed = self.on_continue_button(event.t_ms);
                }
            }
            WizardPhase::Closed => {}
        }

        let finished = matches!(self.phase, WizardPhase::Closed);
        if finished {
            return WizardDispatch::Finished;
        }

        // Up and Swipe are emitted as a pair for a classified gesture. Rendering
        // the raw trace on Up blocks the display loop before it can consume the
        // paired Swipe, then causes a second refresh when the case advances.
        // Wait for classification and render the final result once.
        let waiting_for_swipe_classification = matches!(phase_before, WizardPhase::SwipeRight)
            && matches!(event.kind, TouchEventKind::Up);
        // The fourth corner is captured from Down's first-contact coordinates.
        // Defer that redraw until Up: refreshing e-paper while the contact is
        // still held can interrupt controller sampling and strand the wizard in
        // its release handshake.
        let waiting_for_calibration_release = matches!(phase_before, WizardPhase::Calibrate)
            && matches!(raw_event.kind, TouchEventKind::Down)
            && self.calibration_pending_return;
        if changed && !waiting_for_swipe_classification && !waiting_for_calibration_release {
            self.render_partial(display).await;
        }
        WizardDispatch::Consumed
    }
}

pub(super) fn pending_release_matches_swipe(
    pending: PendingSwipeRelease,
    event: TouchEvent,
) -> bool {
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
