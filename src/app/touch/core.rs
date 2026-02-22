use statig::{blocking::IntoStateMachineExt as _, prelude::*};

const TOUCH_DEBOUNCE_DOWN_MS: u64 = 30;
const TOUCH_DEBOUNCE_UP_MS: u64 = 35;
const TOUCH_DRAG_START_PX: i32 = 14;
const TOUCH_MOVE_DEADZONE_PX: i32 = 8;
const TOUCH_LONG_PRESS_MS: u64 = 700;
const TOUCH_TAP_MAX_MS: u64 = 280;
const TOUCH_SWIPE_MIN_DISTANCE_PX: i32 = 96;
const TOUCH_SWIPE_MAX_DURATION_MS: u64 = 550;
const TOUCH_SWIPE_AXIS_DOMINANCE_X100: i32 = 140;

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
    down_ms: u64,
    down_point: TouchPoint,
    last_point: TouchPoint,
    last_move_emit_point: TouchPoint,
    release_ms: u64,
    release_point: TouchPoint,
    drag_active: bool,
    long_press_emitted: bool,
}

impl TouchHsm {
    fn new() -> Self {
        Self {
            down_ms: 0,
            down_point: TouchPoint::default(),
            last_point: TouchPoint::default(),
            last_move_emit_point: TouchPoint::default(),
            release_ms: 0,
            release_point: TouchPoint::default(),
            drag_active: false,
            long_press_emitted: false,
        }
    }

    fn begin_press(&mut self, now_ms: u64, point: TouchPoint) {
        self.down_ms = now_ms;
        self.down_point = point;
        self.last_point = point;
        self.last_move_emit_point = point;
        self.release_ms = now_ms;
        self.release_point = point;
        self.drag_active = false;
        self.long_press_emitted = false;
    }

    fn reset_interaction(&mut self) {
        self.drag_active = false;
        self.long_press_emitted = false;
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
        TouchEvent {
            kind,
            t_ms: now_ms,
            x: point.x,
            y: point.y,
            start_x: self.down_point.x,
            start_y: self.down_point.y,
            duration_ms: self.interaction_duration_ms(now_ms),
            touch_count,
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
            self.emit_event(context, TouchEventKind::Move, now_ms, point, 1);
        }
    }

    fn classify_swipe(
        &self,
        release_ms: u64,
        release_point: TouchPoint,
    ) -> Option<TouchSwipeDirection> {
        let duration_ms = release_ms.saturating_sub(self.down_ms);
        if duration_ms > TOUCH_SWIPE_MAX_DURATION_MS {
            return None;
        }

        let dx = release_point.x as i32 - self.down_point.x as i32;
        let dy = release_point.y as i32 - self.down_point.y as i32;
        let abs_dx = dx.abs();
        let abs_dy = dy.abs();
        let major = abs_dx.max(abs_dy);
        let minor = abs_dx.min(abs_dy);

        if major < TOUCH_SWIPE_MIN_DISTANCE_PX {
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

    fn finalize_release(&mut self, context: &mut DispatchContext) {
        let release_ms = self.release_ms;
        let release_point = self.release_point;

        self.emit_event(context, TouchEventKind::Up, release_ms, release_point, 0);

        if self.drag_active {
            if let Some(direction) = self.classify_swipe(release_ms, release_point) {
                self.emit_event(
                    context,
                    TouchEventKind::Swipe(direction),
                    release_ms,
                    release_point,
                    0,
                );
            }
        } else {
            let duration = release_ms.saturating_sub(self.down_ms);
            if !self.long_press_emitted && duration <= TOUCH_TAP_MAX_MS {
                self.emit_event(context, TouchEventKind::Tap, release_ms, release_point, 0);
            }
        }

        self.reset_interaction();
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
                        self.reset_interaction();
                        Transition(State::idle())
                    }
                    (1, Some(point)) => {
                        self.last_point = point;
                        if now_ms.saturating_sub(self.down_ms) >= TOUCH_DEBOUNCE_DOWN_MS {
                            // Anchor the interaction origin after debounce has stabilized.
                            // This avoids swipe/drag bias from a noisy first contact sample.
                            self.down_point = point;
                            self.last_move_emit_point = point;
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
                        self.last_point = point;
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
                        self.last_point = point;
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
                match (count, point) {
                    (0, _) => {
                        if now_ms.saturating_sub(self.release_ms) >= TOUCH_DEBOUNCE_UP_MS {
                            self.finalize_release(context);
                            Transition(State::idle())
                        } else {
                            Handled
                        }
                    }
                    (1, Some(point)) => {
                        if now_ms.saturating_sub(self.release_ms) < TOUCH_DEBOUNCE_UP_MS {
                            self.last_point = point;
                            if self.drag_active {
                                Transition(State::dragging())
                            } else {
                                Transition(State::pressed())
                            }
                        } else {
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
}
