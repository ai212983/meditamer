use super::*;

impl TouchHsm {
    pub(in super::super) fn finalize_release(&mut self, context: &mut DispatchContext) {
        let release_ms = self.release_ms;
        let release_point = self.release_point;
        self.interaction_release_debounce_ms =
            self.release_debounce_ms().min(u16::MAX as u64) as u16;
        let swipe_point =
            if self.farthest_distance_sq > squared_distance(release_point, self.down_point) {
                self.farthest_point
            } else {
                release_point
            };

        self.emit_event(context, TouchEventKind::Up, release_ms, release_point, 0);

        // Classify swipe from the furthest observed interaction point.
        // This preserves real swipes when release samples jitter near the start.
        if let Some(direction) = self.classify_swipe(release_ms, swipe_point) {
            self.arm_post_swipe_guard(release_ms, release_point);
            self.emit_event(
                context,
                TouchEventKind::Swipe(direction),
                release_ms,
                swipe_point,
                0,
            );
        } else {
            self.clear_post_swipe_guard();
            let duration = release_ms.saturating_sub(self.down_ms);
            let travel_sq = squared_distance(release_point, self.down_point);
            let tap_travel_sq = TOUCH_TAP_MAX_TRAVEL_PX * TOUCH_TAP_MAX_TRAVEL_PX;
            if !self.long_press_emitted
                && duration <= TOUCH_TAP_MAX_MS
                && travel_sq <= tap_travel_sq
            {
                self.emit_event(context, TouchEventKind::Tap, release_ms, release_point, 0);
            }
        }

        self.reset_interaction();
    }

    pub(in super::super) fn release_debounce_ms(&self) -> u64 {
        // During established drag/swipe motion, tolerate slightly longer zero-count
        // flicker so one physical swipe is not split into two interactions.
        if self.drag_active {
            let mid_sq = 120 * 120;
            let long_sq = 220 * 220;
            if self.farthest_distance_sq >= long_sq {
                TOUCH_DEBOUNCE_UP_DRAG_LONG_MS
            } else if self.farthest_distance_sq >= mid_sq {
                TOUCH_DEBOUNCE_UP_DRAG_MID_MS
            } else {
                TOUCH_DEBOUNCE_UP_DRAG_MS
            }
        } else if self.interaction_move_count == 0
            && self.release_ms.saturating_sub(self.down_ms)
                >= TOUCH_DEBOUNCE_UP_NO_MOVE_MIN_DURATION_MS
        {
            let interaction_age = self.release_ms.saturating_sub(self.down_ms);
            // Treat tiny pre-release jitter as "no motion". Sparse controllers
            // often report 1-2 noisy coordinate deltas before a real swipe frame.
            let tiny_motion_px = TOUCH_MOVE_DEADZONE_PX;
            let pre_motion_absent = self.farthest_distance_sq <= squared_i32(tiny_motion_px)
                && self.interaction_total_path_px as i32 <= tiny_motion_px
                && self.interaction_sum_dx.abs() <= tiny_motion_px
                && self.interaction_sum_dy.abs() <= tiny_motion_px;
            if pre_motion_absent && interaction_age <= TOUCH_DEBOUNCE_UP_NO_MOVE_EARLY_MAX_AGE_MS {
                // Some panels report sparse touch frames at gesture start
                // (single contact frame followed by transient zeros). Keep
                // early no-move releases open longer so the next motion frame
                // can continue the same interaction.
                TOUCH_DEBOUNCE_UP_NO_MOVE_EARLY_MS
            } else {
                // For older no-move interactions keep tap latency low.
                TOUCH_DEBOUNCE_UP_NO_MOVE_MS
            }
        } else {
            TOUCH_DEBOUNCE_UP_MS
        }
    }

    pub(in super::super) fn interaction_origin_point(&self) -> TouchPoint {
        let use_stable_down_origin = squared_distance(self.origin_point, self.down_point)
            >= TOUCH_SWIPE_ORIGIN_NOISE_PX * TOUCH_SWIPE_ORIGIN_NOISE_PX;
        if use_stable_down_origin {
            self.down_point
        } else {
            self.origin_point
        }
    }

    pub(in super::super) fn should_resume_from_release(
        &self,
        now_ms: u64,
        point: TouchPoint,
        debounce_window_ms: u64,
    ) -> bool {
        let elapsed = now_ms.saturating_sub(self.release_ms);
        if elapsed <= debounce_window_ms {
            return true;
        }
        if elapsed > TOUCH_RELEASE_RECONTACT_MAX_GAP_MS {
            return false;
        }

        let near_release = squared_distance(point, self.release_point)
            <= squared_i32(TOUCH_RELEASE_RECONTACT_NEAR_PX);
        if near_release {
            return true;
        }

        if self.drag_active {
            let progressed_from_down = squared_distance(point, self.down_point)
                + squared_i32(TOUCH_RELEASE_PROGRESS_MARGIN_PX)
                >= self.farthest_distance_sq.max(0);
            let near_farthest = squared_distance(point, self.farthest_point)
                <= squared_i32(TOUCH_RELEASE_FARTHEST_NEAR_PX);
            return progressed_from_down || near_farthest;
        }

        if self.interaction_move_count == 0 {
            let interaction_age = now_ms.saturating_sub(self.down_ms);
            let dx = point.x as i32 - self.down_point.x as i32;
            let dy = point.y as i32 - self.down_point.y as i32;
            let dist_sq = squared_distance(point, self.down_point);
            let moved_min = dist_sq >= squared_i32(TOUCH_RELEASE_NO_MOVE_RECOVER_MIN_DISTANCE_PX);
            // When the first stable re-contact after a sparse release appears late,
            // allow a larger jump envelope so long swipes don't split.
            let dynamic_max_distance = TOUCH_RELEASE_NO_MOVE_RECOVER_MAX_DISTANCE_PX
                .saturating_add(
                    ((TOUCH_RELEASE_NO_MOVE_RECOVER_DISTANCE_PER_MS_X100 as u64 * interaction_age)
                        / 100) as i32,
                )
                .min(TOUCH_RELEASE_NO_MOVE_RECOVER_HARD_MAX_DISTANCE_PX);
            let moved_max = dist_sq <= squared_i32(dynamic_max_distance);
            let axis_dominant =
                is_axis_dominant(dx, dy, TOUCH_RELEASE_NO_MOVE_RECOVER_AXIS_DOM_X100);
            if interaction_age <= TOUCH_RELEASE_NO_MOVE_RECOVER_MAX_AGE_MS
                && moved_min
                && moved_max
                && axis_dominant
            {
                return true;
            }
        }

        false
    }

    pub(in super::super) fn should_preserve_pre_debounce_motion(
        &self,
        now_ms: u64,
        point: TouchPoint,
    ) -> bool {
        if now_ms.saturating_sub(self.down_ms) > TOUCH_PRESERVE_PRE_DEBOUNCE_MAX_ELAPSED_MS {
            return false;
        }
        let preserve_sq = squared_i32(TOUCH_PRESERVE_PRE_DEBOUNCE_MOTION_PX);
        squared_distance(self.origin_point, point) >= preserve_sq
            || self.farthest_distance_sq >= preserve_sq
            || self.interaction_total_path_px as i32 >= TOUCH_PRESERVE_PRE_DEBOUNCE_MOTION_PX
    }
}
