use super::*;

impl TouchHsm {
    pub(crate) fn new() -> Self {
        Self {
            origin_point: TouchPoint::default(),
            down_ms: 0,
            down_point: TouchPoint::default(),
            last_point: TouchPoint::default(),
            last_move_emit_point: TouchPoint::default(),
            farthest_point: TouchPoint::default(),
            farthest_distance_sq: 0,
            release_ms: 0,
            release_point: TouchPoint::default(),
            drag_active: false,
            long_press_emitted: false,
            interaction_move_count: 0,
            interaction_total_path_px: 0,
            interaction_sum_dx: 0,
            interaction_sum_dy: 0,
            interaction_peak_speed_x100: 0,
            last_motion_ms: 0,
            interaction_release_debounce_ms: 0,
            interaction_dropout_count: 0,
            post_swipe_guard_active: false,
            post_swipe_guard_until_ms: 0,
            post_swipe_guard_point: TouchPoint::default(),
        }
    }

    pub(super) fn begin_press(&mut self, now_ms: u64, point: TouchPoint) {
        self.origin_point = point;
        self.down_ms = now_ms;
        self.down_point = point;
        self.last_point = point;
        self.last_move_emit_point = point;
        self.farthest_point = point;
        self.farthest_distance_sq = 0;
        self.release_ms = now_ms;
        self.release_point = point;
        self.drag_active = false;
        self.long_press_emitted = false;
        self.interaction_move_count = 0;
        self.interaction_total_path_px = 0;
        self.interaction_sum_dx = 0;
        self.interaction_sum_dy = 0;
        self.interaction_peak_speed_x100 = 0;
        self.last_motion_ms = now_ms;
        self.interaction_release_debounce_ms = 0;
        self.interaction_dropout_count = 0;
    }

    pub(super) fn reset_interaction(&mut self) {
        self.drag_active = false;
        self.long_press_emitted = false;
        self.interaction_move_count = 0;
        self.interaction_total_path_px = 0;
        self.interaction_sum_dx = 0;
        self.interaction_sum_dy = 0;
        self.interaction_peak_speed_x100 = 0;
        self.last_motion_ms = 0;
        self.interaction_release_debounce_ms = 0;
        self.interaction_dropout_count = 0;
    }

    pub(super) fn observe_point(&mut self, now_ms: u64, point: TouchPoint) {
        let prev = self.last_point;
        if point != prev {
            let dx = point.x as i32 - prev.x as i32;
            let dy = point.y as i32 - prev.y as i32;
            let dist_sq = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
            if dist_sq > 0 {
                let step_px = int_sqrt_i32(dist_sq);
                self.interaction_total_path_px = self
                    .interaction_total_path_px
                    .saturating_add(step_px.max(0).min(u16::MAX as i32) as u16);
                self.interaction_sum_dx = self.interaction_sum_dx.saturating_add(dx);
                self.interaction_sum_dy = self.interaction_sum_dy.saturating_add(dy);

                let dt_ms = now_ms.saturating_sub(self.last_motion_ms).max(1);
                let speed_x100 = ((step_px.max(0) as u64).saturating_mul(100))
                    .saturating_div(dt_ms)
                    .min(u16::MAX as u64) as u16;
                if speed_x100 > self.interaction_peak_speed_x100 {
                    self.interaction_peak_speed_x100 = speed_x100;
                }
            }
        }
        self.last_point = point;
        self.last_motion_ms = now_ms;
        self.update_swipe_extent(point);
    }

    pub(super) fn interaction_duration_ms(&self, now_ms: u64) -> u16 {
        now_ms.saturating_sub(self.down_ms).min(u16::MAX as u64) as u16
    }

    pub(super) fn build_event(
        &self,
        kind: TouchEventKind,
        now_ms: u64,
        point: TouchPoint,
        touch_count: u8,
    ) -> TouchEvent {
        let start = self.interaction_origin_point();
        TouchEvent {
            kind,
            t_ms: now_ms,
            x: point.x,
            y: point.y,
            start_x: start.x,
            start_y: start.y,
            duration_ms: self.interaction_duration_ms(now_ms),
            touch_count,
            move_count: self.interaction_move_count,
            max_travel_px: self.interaction_max_travel_px(),
            release_debounce_ms: self.interaction_release_debounce_ms,
            dropout_count: self.interaction_dropout_count,
        }
    }

    pub(super) fn emit_event(
        &self,
        context: &mut DispatchContext,
        kind: TouchEventKind,
        now_ms: u64,
        point: TouchPoint,
        touch_count: u8,
    ) {
        context.emit(self.build_event(kind, now_ms, point, touch_count));
    }

    pub(super) fn emit_cancel(
        &mut self,
        context: &mut DispatchContext,
        now_ms: u64,
        touch_count: u8,
        point: Option<TouchPoint>,
    ) {
        let p = point.unwrap_or(self.last_point);
        self.emit_event(context, TouchEventKind::Cancel, now_ms, p, touch_count);
        self.reset_interaction();
    }

    pub(super) fn maybe_emit_move(
        &mut self,
        context: &mut DispatchContext,
        now_ms: u64,
        point: TouchPoint,
        force: bool,
    ) {
        if force
            || squared_distance(point, self.last_move_emit_point)
                >= TOUCH_MOVE_DEADZONE_PX * TOUCH_MOVE_DEADZONE_PX
        {
            self.last_move_emit_point = point;
            self.interaction_move_count = self.interaction_move_count.saturating_add(1);
            self.emit_event(context, TouchEventKind::Move, now_ms, point, 1);
        }
    }

    pub(super) fn interaction_max_travel_px(&self) -> u16 {
        let sq = self.farthest_distance_sq.max(0);
        int_sqrt_i32(sq).min(u16::MAX as i32) as u16
    }

    pub(super) fn update_swipe_extent(&mut self, point: TouchPoint) {
        let distance_sq = squared_distance(point, self.down_point);
        if distance_sq > self.farthest_distance_sq {
            self.farthest_distance_sq = distance_sq;
            self.farthest_point = point;
        }
    }

    pub(super) fn classify_swipe(
        &self,
        release_ms: u64,
        release_point: TouchPoint,
    ) -> Option<TouchSwipeDirection> {
        let duration_ms = release_ms.saturating_sub(self.down_ms);
        // A confirmed drag with clear travel should still classify as swipe
        // even when the finger stays down longer.
        if !self.drag_active && duration_ms > TOUCH_SWIPE_MAX_DURATION_MS {
            return None;
        }

        // Prefer raw press origin for fast-swipe recovery, but if origin and
        // stabilized down-point diverge too much treat origin as noisy.
        let origin = self.interaction_origin_point();

        let disp_dx = release_point.x as i32 - origin.x as i32;
        let disp_dy = release_point.y as i32 - origin.y as i32;
        let motion_dx = self.interaction_sum_dx;
        let motion_dy = self.interaction_sum_dy;

        // Prefer the stronger signal between end-point displacement and integrated
        // path direction so early/late jitter does not erase real swipe intent.
        let dx = if motion_dx.abs() > disp_dx.abs() {
            motion_dx
        } else {
            disp_dx
        };
        let dy = if motion_dy.abs() > disp_dy.abs() {
            motion_dy
        } else {
            disp_dy
        };

        let abs_dx = dx.abs();
        let abs_dy = dy.abs();
        let major = abs_dx.max(abs_dy);
        let minor = abs_dx.min(abs_dy);
        let path_px = self.interaction_total_path_px as i32;

        let strong_displacement = major >= TOUCH_SWIPE_MIN_DISTANCE_PX;
        let strong_path =
            path_px >= TOUCH_SWIPE_MIN_PATH_PX && major >= TOUCH_SWIPE_MIN_NET_DISTANCE_PX;
        if !strong_displacement && !strong_path {
            return None;
        }

        if major * 100 < minor * TOUCH_SWIPE_AXIS_DOMINANCE_X100 {
            return None;
        }

        if abs_dx >= abs_dy {
            if dx >= 0 {
                Some(TouchSwipeDirection::Right)
            } else {
                Some(TouchSwipeDirection::Left)
            }
        } else if dy >= 0 {
            Some(TouchSwipeDirection::Down)
        } else {
            Some(TouchSwipeDirection::Up)
        }
    }

    pub(super) fn clear_post_swipe_guard(&mut self) {
        self.post_swipe_guard_active = false;
    }

    pub(super) fn arm_post_swipe_guard(&mut self, release_ms: u64, point: TouchPoint) {
        self.post_swipe_guard_active = true;
        self.post_swipe_guard_until_ms = release_ms.saturating_add(TOUCH_POST_SWIPE_REARM_MS);
        self.post_swipe_guard_point = point;
    }

    pub(super) fn suppress_post_swipe_retouch(&mut self, now_ms: u64, point: TouchPoint) -> bool {
        if !self.post_swipe_guard_active {
            return false;
        }
        if now_ms > self.post_swipe_guard_until_ms {
            self.clear_post_swipe_guard();
            return false;
        }
        if squared_distance(point, self.post_swipe_guard_point)
            <= TOUCH_POST_SWIPE_REARM_RADIUS_PX * TOUCH_POST_SWIPE_REARM_RADIUS_PX
        {
            return true;
        }
        self.clear_post_swipe_guard();
        false
    }

    pub(super) fn finalize_release(&mut self, context: &mut DispatchContext) {
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

    pub(super) fn release_debounce_ms(&self) -> u64 {
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

    pub(super) fn interaction_origin_point(&self) -> TouchPoint {
        let use_stable_down_origin = squared_distance(self.origin_point, self.down_point)
            >= TOUCH_SWIPE_ORIGIN_NOISE_PX * TOUCH_SWIPE_ORIGIN_NOISE_PX;
        if use_stable_down_origin {
            self.down_point
        } else {
            self.origin_point
        }
    }

    pub(super) fn should_resume_from_release(
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

    pub(super) fn should_preserve_pre_debounce_motion(
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
