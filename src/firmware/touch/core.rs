use statig::{blocking::IntoStateMachineExt as _, prelude::*};

const TOUCH_DEBOUNCE_DOWN_MS: u64 = 12;
const TOUCH_DEBOUNCE_UP_MS: u64 = 16;
const TOUCH_DEBOUNCE_UP_NO_MOVE_MS: u64 = 32;
const TOUCH_DEBOUNCE_UP_NO_MOVE_EARLY_MS: u64 = 112;
const TOUCH_DEBOUNCE_UP_NO_MOVE_EARLY_MAX_AGE_MS: u64 = 64;
const TOUCH_DEBOUNCE_UP_NO_MOVE_MIN_DURATION_MS: u64 = 24;
const TOUCH_DEBOUNCE_UP_DRAG_MS: u64 = 32;
const TOUCH_DEBOUNCE_UP_DRAG_MID_MS: u64 = 56;
const TOUCH_DEBOUNCE_UP_DRAG_LONG_MS: u64 = 84;
const TOUCH_RELEASE_RECONTACT_MAX_GAP_MS: u64 = 180;
const TOUCH_RELEASE_RECONTACT_NEAR_PX: i32 = 36;
const TOUCH_RELEASE_NO_MOVE_RECOVER_MAX_AGE_MS: u64 = 220;
const TOUCH_RELEASE_NO_MOVE_RECOVER_MIN_DISTANCE_PX: i32 = 48;
const TOUCH_RELEASE_NO_MOVE_RECOVER_MAX_DISTANCE_PX: i32 = 300;
const TOUCH_RELEASE_NO_MOVE_RECOVER_DISTANCE_PER_MS_X100: i32 = 220;
const TOUCH_RELEASE_NO_MOVE_RECOVER_HARD_MAX_DISTANCE_PX: i32 = 560;
const TOUCH_RELEASE_NO_MOVE_RECOVER_AXIS_DOM_X100: i32 = 130;
const TOUCH_RELEASE_PROGRESS_MARGIN_PX: i32 = 24;
const TOUCH_RELEASE_FARTHEST_NEAR_PX: i32 = 64;
const TOUCH_DEBOUNCE_DOWN_ABORT_MS: u64 = 40;
const TOUCH_DRAG_START_PX: i32 = 10;
const TOUCH_PRESERVE_PRE_DEBOUNCE_MOTION_PX: i32 = 24;
const TOUCH_PRESERVE_PRE_DEBOUNCE_MAX_ELAPSED_MS: u64 = 24;
const TOUCH_MOVE_DEADZONE_PX: i32 = 6;
const TOUCH_LONG_PRESS_MS: u64 = 700;
const TOUCH_TAP_MAX_MS: u64 = 280;
const TOUCH_TAP_MAX_TRAVEL_PX: i32 = 24;
const TOUCH_SWIPE_MIN_DISTANCE_PX: i32 = 40;
const TOUCH_SWIPE_MIN_NET_DISTANCE_PX: i32 = 24;
const TOUCH_SWIPE_MIN_PATH_PX: i32 = 56;
const TOUCH_SWIPE_MAX_DURATION_MS: u64 = 1_000;
const TOUCH_SWIPE_AXIS_DOMINANCE_X100: i32 = 110;
const TOUCH_SWIPE_ORIGIN_NOISE_PX: i32 = 64;
const TOUCH_POST_SWIPE_REARM_MS: u64 = 140;
const TOUCH_POST_SWIPE_REARM_RADIUS_PX: i32 = 18;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TouchSample {
    pub touch_count: u8,
    pub points: [TouchPoint; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchSwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchEventKind {
    Down,
    Move,
    Up,
    Tap,
    LongPress,
    Swipe(TouchSwipeDirection),
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchEvent {
    pub kind: TouchEventKind,
    pub t_ms: u64,
    pub x: u16,
    pub y: u16,
    pub start_x: u16,
    pub start_y: u16,
    pub duration_ms: u16,
    pub touch_count: u8,
    pub move_count: u16,
    pub max_travel_px: u16,
    pub release_debounce_ms: u16,
    pub dropout_count: u16,
}

#[derive(Clone, Copy, Debug)]
enum TouchHsmEvent {
    Sample { now_ms: u64, sample: TouchSample },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TouchEngineOutput {
    pub events: [Option<TouchEvent>; 3],
}

#[derive(Clone, Copy, Debug, Default)]
struct DispatchContext {
    events: [Option<TouchEvent>; 3],
}

impl DispatchContext {
    fn emit(&mut self, event: TouchEvent) {
        for slot in &mut self.events {
            if slot.is_none() {
                *slot = Some(event);
                return;
            }
        }
    }

    fn finish(self) -> TouchEngineOutput {
        TouchEngineOutput {
            events: self.events,
        }
    }
}

pub struct TouchEngine {
    machine: statig::blocking::StateMachine<TouchHsm>,
}

impl Default for TouchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TouchEngine {
    pub fn new() -> Self {
        Self {
            machine: TouchHsm::new().state_machine(),
        }
    }

    pub fn tick(&mut self, now_ms: u64, sample: TouchSample) -> TouchEngineOutput {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&TouchHsmEvent::Sample { now_ms, sample }, &mut context);
        context.finish()
    }
}

struct TouchHsm {
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

impl TouchHsm {
    fn new() -> Self {
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

    fn begin_press(&mut self, now_ms: u64, point: TouchPoint) {
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

    fn reset_interaction(&mut self) {
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

    fn observe_point(&mut self, now_ms: u64, point: TouchPoint) {
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

    fn interaction_duration_ms(&self, now_ms: u64) -> u16 {
        now_ms.saturating_sub(self.down_ms).min(u16::MAX as u64) as u16
    }

    fn build_event(
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

    fn emit_event(
        &self,
        context: &mut DispatchContext,
        kind: TouchEventKind,
        now_ms: u64,
        point: TouchPoint,
        touch_count: u8,
    ) {
        context.emit(self.build_event(kind, now_ms, point, touch_count));
    }

    fn emit_cancel(
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

    fn maybe_emit_move(
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

    fn interaction_max_travel_px(&self) -> u16 {
        let sq = self.farthest_distance_sq.max(0);
        int_sqrt_i32(sq).min(u16::MAX as i32) as u16
    }

    fn update_swipe_extent(&mut self, point: TouchPoint) {
        let distance_sq = squared_distance(point, self.down_point);
        if distance_sq > self.farthest_distance_sq {
            self.farthest_distance_sq = distance_sq;
            self.farthest_point = point;
        }
    }

    fn classify_swipe(
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

    fn clear_post_swipe_guard(&mut self) {
        self.post_swipe_guard_active = false;
    }

    fn arm_post_swipe_guard(&mut self, release_ms: u64, point: TouchPoint) {
        self.post_swipe_guard_active = true;
        self.post_swipe_guard_until_ms = release_ms.saturating_add(TOUCH_POST_SWIPE_REARM_MS);
        self.post_swipe_guard_point = point;
    }

    fn suppress_post_swipe_retouch(&mut self, now_ms: u64, point: TouchPoint) -> bool {
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

    fn finalize_release(&mut self, context: &mut DispatchContext) {
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

    fn release_debounce_ms(&self) -> u64 {
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

    fn interaction_origin_point(&self) -> TouchPoint {
        let use_stable_down_origin = squared_distance(self.origin_point, self.down_point)
            >= TOUCH_SWIPE_ORIGIN_NOISE_PX * TOUCH_SWIPE_ORIGIN_NOISE_PX;
        if use_stable_down_origin {
            self.down_point
        } else {
            self.origin_point
        }
    }

    fn should_resume_from_release(
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

    fn should_preserve_pre_debounce_motion(&self, now_ms: u64, point: TouchPoint) -> bool {
        if now_ms.saturating_sub(self.down_ms) > TOUCH_PRESERVE_PRE_DEBOUNCE_MAX_ELAPSED_MS {
            return false;
        }
        let preserve_sq = squared_i32(TOUCH_PRESERVE_PRE_DEBOUNCE_MOTION_PX);
        squared_distance(self.origin_point, point) >= preserve_sq
            || self.farthest_distance_sq >= preserve_sq
            || self.interaction_total_path_px as i32 >= TOUCH_PRESERVE_PRE_DEBOUNCE_MOTION_PX
    }
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

fn sample_primary(sample: &TouchSample) -> (u8, Option<TouchPoint>) {
    if sample.touch_count == 0 {
        (0, None)
    } else {
        (sample.touch_count, Some(sample.points[0]))
    }
}

fn squared_distance(a: TouchPoint, b: TouchPoint) -> i32 {
    let dx = a.x as i32 - b.x as i32;
    let dy = a.y as i32 - b.y as i32;
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

fn squared_i32(value: i32) -> i32 {
    value.saturating_mul(value)
}

fn is_axis_dominant(dx: i32, dy: i32, ratio_x100: i32) -> bool {
    let ax = dx.abs();
    let ay = dy.abs();
    let major = ax.max(ay);
    let minor = ax.min(ay);
    major > 0 && major.saturating_mul(100) >= minor.saturating_mul(ratio_x100)
}

fn int_sqrt_i32(value: i32) -> i32 {
    if value <= 0 {
        return 0;
    }
    let mut lo = 0i32;
    let mut hi = value.min(46_340) + 1;
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if mid.saturating_mul(mid) <= value {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample1(x: u16, y: u16) -> TouchSample {
        TouchSample {
            touch_count: 1,
            points: [TouchPoint { x, y }, TouchPoint::default()],
        }
    }

    fn sample2(x0: u16, y0: u16, x1: u16, y1: u16) -> TouchSample {
        TouchSample {
            touch_count: 2,
            points: [TouchPoint { x: x0, y: y0 }, TouchPoint { x: x1, y: y1 }],
        }
    }

    fn sample0() -> TouchSample {
        TouchSample {
            touch_count: 0,
            points: [TouchPoint::default(), TouchPoint::default()],
        }
    }

    fn drain_kinds(output: TouchEngineOutput, out: &mut std::vec::Vec<TouchEventKind>) {
        for event in output.events.into_iter().flatten() {
            out.push(event.kind);
        }
    }

    #[test]
    fn tap_emits_down_up_tap() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(100, 120)), &mut events);
        drain_kinds(engine.tick(20, sample1(100, 120)), &mut events);
        drain_kinds(engine.tick(35, sample1(101, 120)), &mut events);
        drain_kinds(engine.tick(90, sample1(101, 121)), &mut events);
        drain_kinds(engine.tick(110, sample0()), &mut events);
        drain_kinds(engine.tick(150, sample0()), &mut events);

        assert_eq!(
            events,
            std::vec![
                TouchEventKind::Down,
                TouchEventKind::Up,
                TouchEventKind::Tap
            ]
        );
    }

    #[test]
    fn long_press_emits_without_tap() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(200, 200)), &mut events);
        drain_kinds(engine.tick(35, sample1(200, 200)), &mut events);
        drain_kinds(engine.tick(760, sample1(201, 200)), &mut events);
        drain_kinds(engine.tick(800, sample0()), &mut events);
        drain_kinds(engine.tick(840, sample0()), &mut events);

        assert_eq!(
            events,
            std::vec![
                TouchEventKind::Down,
                TouchEventKind::LongPress,
                TouchEventKind::Up
            ]
        );
    }

    #[test]
    fn swipe_right_emits_move_up_swipe() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(50, 100)), &mut events);
        drain_kinds(engine.tick(35, sample1(50, 100)), &mut events);
        drain_kinds(engine.tick(80, sample1(90, 103)), &mut events);
        drain_kinds(engine.tick(120, sample1(180, 108)), &mut events);
        drain_kinds(engine.tick(150, sample0()), &mut events);
        drain_kinds(engine.tick(190, sample0()), &mut events);
        drain_kinds(engine.tick(230, sample0()), &mut events);

        assert_eq!(events[0], TouchEventKind::Down);
        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Move)));
        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
        assert!(events
            .iter()
            .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
    }

    #[test]
    fn multitouch_cancels_current_interaction() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(120, 120)), &mut events);
        drain_kinds(engine.tick(35, sample1(120, 120)), &mut events);
        drain_kinds(engine.tick(60, sample2(121, 121, 220, 220)), &mut events);
        drain_kinds(engine.tick(100, sample0()), &mut events);
        drain_kinds(engine.tick(160, sample0()), &mut events);

        assert_eq!(events[0], TouchEventKind::Down);
        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Cancel)));
        assert!(!events.iter().any(|k| matches!(k, TouchEventKind::Tap)));
    }

    #[test]
    fn brief_bounce_is_ignored_by_down_debounce() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(20, 20)), &mut events);
        drain_kinds(engine.tick(10, sample0()), &mut events);
        drain_kinds(engine.tick(60, sample0()), &mut events);

        assert!(events.is_empty());
    }

    #[test]
    fn down_origin_is_anchored_after_debounce() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        // First sample is noisy; stabilized point appears by debounce boundary.
        let _ = engine.tick(0, sample1(40, 40));
        let output = engine.tick(35, sample1(100, 120));
        for event in output.events.into_iter().flatten() {
            events.push(event);
        }
        let _ = engine.tick(80, sample0());
        let output = engine.tick(120, sample0());
        for event in output.events.into_iter().flatten() {
            events.push(event);
        }

        let down = events
            .iter()
            .find(|ev| matches!(ev.kind, TouchEventKind::Down))
            .expect("missing down event");
        assert_eq!(down.start_x, 100);
        assert_eq!(down.start_y, 120);

        let up = events
            .iter()
            .find(|ev| matches!(ev.kind, TouchEventKind::Up))
            .expect("missing up event");
        assert_eq!(up.start_x, 100);
        assert_eq!(up.start_y, 120);

        assert!(events
            .iter()
            .any(|ev| matches!(ev.kind, TouchEventKind::Tap)));
    }

    #[test]
    fn jitter_drag_still_emits_tap_when_release_is_near() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        // Jitter briefly crosses drag threshold but total release travel remains tap-like.
        drain_kinds(engine.tick(0, sample1(200, 200)), &mut events);
        drain_kinds(engine.tick(35, sample1(200, 200)), &mut events);
        drain_kinds(engine.tick(70, sample1(212, 205)), &mut events);
        drain_kinds(engine.tick(95, sample0()), &mut events);
        drain_kinds(engine.tick(130, sample0()), &mut events);

        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Down)));
        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Tap)));
    }

    #[test]
    fn short_press_release_during_down_debounce_emits_tap() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        // Press starts, then touch count flickers to zero before a stable second count=1 sample.
        // Engine should still produce a real tap interaction.
        drain_kinds(engine.tick(0, sample1(180, 220)), &mut events);
        drain_kinds(engine.tick(8, sample0()), &mut events);
        drain_kinds(engine.tick(16, sample0()), &mut events);
        drain_kinds(engine.tick(40, sample0()), &mut events);

        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Down)));
        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Tap)));
    }

    #[test]
    fn fast_swipe_during_down_debounce_is_still_detected() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        // Finger moves quickly before debounce-down promotes to Pressed.
        drain_kinds(engine.tick(0, sample1(50, 100)), &mut events);
        drain_kinds(engine.tick(8, sample1(120, 102)), &mut events);
        drain_kinds(engine.tick(16, sample0()), &mut events);
        drain_kinds(engine.tick(40, sample0()), &mut events);

        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Down)));
        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
        assert!(events
            .iter()
            .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
    }

    #[test]
    fn pre_debounce_fast_motion_is_preserved_at_down_promotion() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        // Motion happens before debounce promotion and press remains active at
        // the promotion sample. Engine should preserve early path and classify swipe.
        drain_kinds(engine.tick(0, sample1(70, 220)), &mut events);
        drain_kinds(engine.tick(8, sample1(150, 222)), &mut events);
        drain_kinds(engine.tick(16, sample1(240, 224)), &mut events);
        drain_kinds(engine.tick(24, sample0()), &mut events);
        drain_kinds(engine.tick(80, sample0()), &mut events);

        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Down)));
        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
        assert!(events
            .iter()
            .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
    }

    #[test]
    fn drag_flicker_does_not_split_swipe_into_two_touches() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(40, 120)), &mut events);
        drain_kinds(engine.tick(16, sample1(40, 120)), &mut events);
        drain_kinds(engine.tick(24, sample1(90, 121)), &mut events);
        // Brief count=0 drop while finger is still moving.
        drain_kinds(engine.tick(32, sample0()), &mut events);
        drain_kinds(engine.tick(40, sample0()), &mut events);
        // Recover touch before drag debounce window expires.
        drain_kinds(engine.tick(48, sample1(165, 123)), &mut events);
        drain_kinds(engine.tick(56, sample0()), &mut events);
        drain_kinds(engine.tick(96, sample0()), &mut events);
        drain_kinds(engine.tick(128, sample0()), &mut events);

        assert!(events
            .iter()
            .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
        assert_eq!(
            events
                .iter()
                .filter(|k| matches!(k, TouchEventKind::Down))
                .count(),
            1
        );
    }

    #[test]
    fn swipe_detected_even_if_release_returns_near_start() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(60, 120)), &mut events);
        drain_kinds(engine.tick(16, sample1(60, 120)), &mut events);
        drain_kinds(engine.tick(32, sample1(180, 121)), &mut events);
        // Finger jitters back before lift.
        drain_kinds(engine.tick(48, sample1(90, 122)), &mut events);
        drain_kinds(engine.tick(64, sample0()), &mut events);
        drain_kinds(engine.tick(120, sample0()), &mut events);
        drain_kinds(engine.tick(136, sample0()), &mut events);

        assert!(events
            .iter()
            .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
    }

    #[test]
    fn recontact_after_release_gap_emits_up_for_previous_interaction() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(100, 100)), &mut events);
        drain_kinds(engine.tick(20, sample1(100, 100)), &mut events);
        // Enter release debounce.
        drain_kinds(engine.tick(40, sample0()), &mut events);
        // Re-contact well after continuity recovery window; old press must
        // finalize with Up before a new interaction starts.
        drain_kinds(engine.tick(160, sample1(200, 200)), &mut events);

        assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
    }

    #[test]
    fn no_move_release_recontact_continues_same_swipe() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        // Press stabilizes.
        drain_kinds(engine.tick(0, sample1(100, 100)), &mut events);
        drain_kinds(engine.tick(20, sample1(100, 100)), &mut events);
        // Brief dropout starts release candidate.
        drain_kinds(engine.tick(40, sample0()), &mut events);
        // Re-contact shortly after debounce window, now far enough from down to
        // indicate same swipe continuation rather than a new tap.
        drain_kinds(engine.tick(76, sample1(170, 102)), &mut events);
        drain_kinds(engine.tick(92, sample1(230, 103)), &mut events);
        drain_kinds(engine.tick(120, sample0()), &mut events);
        drain_kinds(engine.tick(180, sample0()), &mut events);

        assert_eq!(
            events
                .iter()
                .filter(|k| matches!(k, TouchEventKind::Down))
                .count(),
            1
        );
        assert!(events
            .iter()
            .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
    }

    #[test]
    fn no_move_release_large_late_recontact_still_continues_swipe() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        // Stabilize press, then drop to zero before any drag sample is observed.
        drain_kinds(engine.tick(0, sample1(100, 100)), &mut events);
        drain_kinds(engine.tick(20, sample1(100, 100)), &mut events);
        drain_kinds(engine.tick(36, sample0()), &mut events);
        // Re-contact arrives after debounce window with a long rightward jump.
        drain_kinds(engine.tick(160, sample1(430, 104)), &mut events);
        drain_kinds(engine.tick(176, sample1(520, 106)), &mut events);
        drain_kinds(engine.tick(208, sample0()), &mut events);
        drain_kinds(engine.tick(320, sample0()), &mut events);

        assert_eq!(
            events
                .iter()
                .filter(|k| matches!(k, TouchEventKind::Down))
                .count(),
            1
        );
        assert!(events
            .iter()
            .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
    }

    #[test]
    fn no_move_release_recontact_beyond_hard_max_starts_new_interaction() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(100, 100)), &mut events);
        drain_kinds(engine.tick(20, sample1(100, 100)), &mut events);
        drain_kinds(engine.tick(36, sample0()), &mut events);
        // Too far from original down point to be treated as same interaction.
        drain_kinds(engine.tick(160, sample1(760, 100)), &mut events);
        drain_kinds(engine.tick(192, sample1(760, 100)), &mut events);

        let down_count = events
            .iter()
            .filter(|k| matches!(k, TouchEventKind::Down))
            .count();
        assert!(down_count >= 2);
    }

    #[test]
    fn tiny_jitter_no_move_release_uses_extended_debounce() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        // Stabilize press with tiny jitter that should still count as "no motion".
        let _ = engine.tick(0, sample1(200, 220));
        for event in engine
            .tick(16, sample1(201, 220))
            .events
            .into_iter()
            .flatten()
        {
            events.push(event);
        }
        // Enter release debounce; no re-contact.
        for event in engine.tick(24, sample0()).events.into_iter().flatten() {
            events.push(event);
        }
        // Should not finalize yet because early no-move debounce should be extended.
        for event in engine.tick(120, sample0()).events.into_iter().flatten() {
            events.push(event);
        }
        assert!(!events
            .iter()
            .any(|ev| matches!(ev.kind, TouchEventKind::Up)));

        // Past extended window, release must finalize with the extended debounce value.
        for event in engine.tick(144, sample0()).events.into_iter().flatten() {
            events.push(event);
        }
        let up = events
            .iter()
            .find(|ev| matches!(ev.kind, TouchEventKind::Up))
            .expect("missing up event");
        assert_eq!(
            up.release_debounce_ms,
            TOUCH_DEBOUNCE_UP_NO_MOVE_EARLY_MS as u16
        );
    }

    #[test]
    fn sparse_start_reports_still_recover_into_single_swipe() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        // Touch starts, then panel emits zeros before next coordinate update.
        drain_kinds(engine.tick(0, sample1(100, 120)), &mut events);
        drain_kinds(engine.tick(16, sample1(100, 120)), &mut events);
        drain_kinds(engine.tick(32, sample0()), &mut events);
        drain_kinds(engine.tick(64, sample0()), &mut events);
        drain_kinds(engine.tick(96, sample0()), &mut events);
        // Sparse re-contact still belongs to same physical gesture.
        drain_kinds(engine.tick(128, sample1(185, 122)), &mut events);
        drain_kinds(engine.tick(160, sample1(245, 123)), &mut events);
        drain_kinds(engine.tick(176, sample0()), &mut events);
        drain_kinds(engine.tick(320, sample0()), &mut events);

        assert_eq!(
            events
                .iter()
                .filter(|k| matches!(k, TouchEventKind::Down))
                .count(),
            1
        );
        assert!(events
            .iter()
            .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
    }

    #[test]
    fn slow_drag_still_emits_swipe_when_travel_is_clear() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(80, 160)), &mut events);
        drain_kinds(engine.tick(20, sample1(80, 160)), &mut events);
        // Slow but clear rightward drag lasting longer than swipe max duration.
        drain_kinds(engine.tick(700, sample1(170, 162)), &mut events);
        drain_kinds(engine.tick(1_220, sample1(245, 164)), &mut events);
        drain_kinds(engine.tick(1_260, sample0()), &mut events);
        drain_kinds(engine.tick(1_360, sample0()), &mut events);

        assert!(events
            .iter()
            .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
    }

    #[test]
    fn post_swipe_retouch_near_release_is_suppressed() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        // Complete a normal swipe.
        drain_kinds(engine.tick(0, sample1(120, 220)), &mut events);
        drain_kinds(engine.tick(16, sample1(120, 220)), &mut events);
        drain_kinds(engine.tick(64, sample1(220, 222)), &mut events);
        drain_kinds(engine.tick(96, sample0()), &mut events);
        drain_kinds(engine.tick(136, sample0()), &mut events);

        let down_before = events
            .iter()
            .filter(|k| matches!(k, TouchEventKind::Down))
            .count();

        // Controller reports a short follow-up contact near release point.
        drain_kinds(engine.tick(176, sample1(223, 223)), &mut events);
        drain_kinds(engine.tick(200, sample0()), &mut events);
        drain_kinds(engine.tick(240, sample0()), &mut events);

        let down_after = events
            .iter()
            .filter(|k| matches!(k, TouchEventKind::Down))
            .count();

        assert_eq!(down_before, down_after);
        assert!(!events.iter().any(|k| matches!(k, TouchEventKind::Tap)));
        assert!(events
            .iter()
            .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
    }

    #[test]
    fn post_swipe_new_touch_far_from_release_starts_new_interaction() {
        let mut engine = TouchEngine::new();
        let mut events = std::vec::Vec::new();

        drain_kinds(engine.tick(0, sample1(80, 180)), &mut events);
        drain_kinds(engine.tick(16, sample1(80, 180)), &mut events);
        drain_kinds(engine.tick(64, sample1(180, 182)), &mut events);
        drain_kinds(engine.tick(96, sample0()), &mut events);
        drain_kinds(engine.tick(136, sample0()), &mut events);

        // New touch far away should not be suppressed by post-swipe guard.
        drain_kinds(engine.tick(176, sample1(300, 300)), &mut events);
        drain_kinds(engine.tick(208, sample1(300, 300)), &mut events);

        let down_count = events
            .iter()
            .filter(|k| matches!(k, TouchEventKind::Down))
            .count();
        assert!(down_count >= 2);
    }
}
