use super::utils::{int_sqrt_i32, is_axis_dominant, sample_primary, squared_distance, squared_i32};
use super::*;
use statig::prelude::*;

mod core;

pub(super) struct TouchHsm {
    origin_point: TouchPoint,
    down_ms: u64,
    down_point: TouchPoint,
    last_point: TouchPoint,
    last_move_emit_point: TouchPoint,
    farthest_point: TouchPoint,
    farthest_distance_sq: i32,
    release_ms: u64,
    release_point: TouchPoint,
    drag_active: bool,
    long_press_emitted: bool,
    interaction_move_count: u16,
    interaction_total_path_px: u16,
    interaction_sum_dx: i32,
    interaction_sum_dy: i32,
    interaction_peak_speed_x100: u16,
    last_motion_ms: u64,
    interaction_release_debounce_ms: u16,
    interaction_dropout_count: u16,
    post_swipe_guard_active: bool,
    post_swipe_guard_until_ms: u64,
    post_swipe_guard_point: TouchPoint,
}

#[state_machine(initial = "State::idle()")]
impl TouchHsm {
    #[state]
    fn idle(&mut self, context: &mut DispatchContext, event: &TouchHsmEvent) -> Outcome<State> {
        let _ = context;
        match event {
            TouchHsmEvent::Sample { now_ms, sample } => {
                let (count, point) = sample_primary(sample);
                if count == 1 {
                    if let Some(point) = point {
                        if self.suppress_post_swipe_retouch(*now_ms, point) {
                            return Handled;
                        }
                        self.begin_press(*now_ms, point);
                        return Transition(State::debounce_down());
                    }
                }
                Handled
            }
        }
    }

    #[state]
    fn debounce_down(
        &mut self,
        context: &mut DispatchContext,
        event: &TouchHsmEvent,
    ) -> Outcome<State> {
        match event {
            TouchHsmEvent::Sample { now_ms, sample } => {
                let (count, point) = sample_primary(sample);
                match (count, point) {
                    (0, _) => {
                        let elapsed = now_ms.saturating_sub(self.down_ms);
                        if (TOUCH_DEBOUNCE_DOWN_MS..=TOUCH_DEBOUNCE_DOWN_ABORT_MS)
                            .contains(&elapsed)
                        {
                            // A short release can happen before we observe another stable `count=1`
                            // sample. Emit Down, then debounce release so quick recovery can
                            // continue as one interaction (important for fast swipes).
                            self.emit_event(
                                context,
                                TouchEventKind::Down,
                                *now_ms,
                                self.last_point,
                                1,
                            );
                            self.release_ms = *now_ms;
                            self.release_point = self.last_point;
                            return Transition(State::debounce_up());
                        }
                        // Some panels briefly drop to zero on first contact.
                        // Keep waiting for a stable press unless the gap persists.
                        if elapsed >= TOUCH_DEBOUNCE_DOWN_ABORT_MS {
                            self.reset_interaction();
                            Transition(State::idle())
                        } else {
                            Handled
                        }
                    }
                    (1, Some(point)) => {
                        self.observe_point(*now_ms, point);
                        if now_ms.saturating_sub(self.down_ms) >= TOUCH_DEBOUNCE_DOWN_MS {
                            if self.should_preserve_pre_debounce_motion(*now_ms, point) {
                                // Keep origin and pre-debounce path when contact already
                                // moved significantly before debounce promotion.
                                self.last_move_emit_point = point;
                            } else {
                                // Anchor the interaction origin after debounce has stabilized.
                                // This avoids swipe/drag bias from a noisy first contact sample.
                                self.down_point = point;
                                self.last_move_emit_point = point;
                                self.farthest_point = point;
                                self.farthest_distance_sq = 0;
                                // Drop pre-debounce motion history once the press is
                                // stabilized so noisy first-contact jumps do not
                                // contaminate normal tap/swipe classification.
                                self.interaction_total_path_px = 0;
                                self.interaction_sum_dx = 0;
                                self.interaction_sum_dy = 0;
                                self.interaction_peak_speed_x100 = 0;
                                self.last_motion_ms = *now_ms;
                            }
                            self.emit_event(context, TouchEventKind::Down, *now_ms, point, 1);
                            Transition(State::pressed())
                        } else {
                            Handled
                        }
                    }
                    _ => {
                        self.emit_cancel(context, *now_ms, count, point);
                        Transition(State::idle())
                    }
                }
            }
        }
    }

    #[state]
    fn pressed(&mut self, context: &mut DispatchContext, event: &TouchHsmEvent) -> Outcome<State> {
        match event {
            TouchHsmEvent::Sample { now_ms, sample } => {
                let (count, point) = sample_primary(sample);
                match (count, point) {
                    (0, _) => {
                        self.release_ms = *now_ms;
                        self.release_point = self.last_point;
                        Transition(State::debounce_up())
                    }
                    (1, Some(point)) => {
                        self.observe_point(*now_ms, point);
                        if squared_distance(point, self.down_point)
                            >= TOUCH_DRAG_START_PX * TOUCH_DRAG_START_PX
                        {
                            self.drag_active = true;
                            self.maybe_emit_move(context, *now_ms, point, true);
                            return Transition(State::dragging());
                        }

                        if !self.long_press_emitted
                            && now_ms.saturating_sub(self.down_ms) >= TOUCH_LONG_PRESS_MS
                        {
                            self.long_press_emitted = true;
                            self.emit_event(context, TouchEventKind::LongPress, *now_ms, point, 1);
                        }

                        Handled
                    }
                    _ => {
                        self.emit_cancel(context, *now_ms, count, point);
                        Transition(State::idle())
                    }
                }
            }
        }
    }

    #[state]
    fn dragging(&mut self, context: &mut DispatchContext, event: &TouchHsmEvent) -> Outcome<State> {
        match event {
            TouchHsmEvent::Sample { now_ms, sample } => {
                let (count, point) = sample_primary(sample);
                match (count, point) {
                    (0, _) => {
                        self.release_ms = *now_ms;
                        self.release_point = self.last_point;
                        Transition(State::debounce_up())
                    }
                    (1, Some(point)) => {
                        self.observe_point(*now_ms, point);
                        self.maybe_emit_move(context, *now_ms, point, false);
                        Handled
                    }
                    _ => {
                        self.emit_cancel(context, *now_ms, count, point);
                        Transition(State::idle())
                    }
                }
            }
        }
    }

    #[state]
    fn debounce_up(
        &mut self,
        context: &mut DispatchContext,
        event: &TouchHsmEvent,
    ) -> Outcome<State> {
        match event {
            TouchHsmEvent::Sample { now_ms, sample } => {
                let (count, point) = sample_primary(sample);
                let debounce_window_ms = self.release_debounce_ms();
                match (count, point) {
                    (0, _) => {
                        if now_ms.saturating_sub(self.release_ms) > debounce_window_ms {
                            self.finalize_release(context);
                            Transition(State::idle())
                        } else {
                            Handled
                        }
                    }
                    (1, Some(point)) => {
                        if self.should_resume_from_release(*now_ms, point, debounce_window_ms) {
                            self.observe_point(*now_ms, point);
                            self.interaction_dropout_count =
                                self.interaction_dropout_count.saturating_add(1);
                            if self.drag_active
                                || squared_distance(point, self.down_point)
                                    >= squared_i32(TOUCH_DRAG_START_PX)
                            {
                                self.drag_active = true;
                                self.maybe_emit_move(context, *now_ms, point, true);
                                Transition(State::dragging())
                            } else {
                                Transition(State::pressed())
                            }
                        } else {
                            // Previous interaction has been released long enough to be
                            // finalized; emit Up/(Tap|Swipe) before starting a new press.
                            self.finalize_release(context);
                            if self.suppress_post_swipe_retouch(*now_ms, point) {
                                return Transition(State::idle());
                            }
                            self.begin_press(*now_ms, point);
                            Transition(State::debounce_down())
                        }
                    }
                    _ => {
                        self.emit_cancel(context, *now_ms, count, point);
                        Transition(State::idle())
                    }
                }
            }
        }
    }
}
