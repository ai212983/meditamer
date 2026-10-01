use statig::blocking::IntoStateMachineExt as _;

mod hsm;
mod utils;

use hsm::TouchHsm;

const TOUCH_DEBOUNCE_DOWN_MS: u64 = 12;
const TOUCH_DEBOUNCE_UP_MS: u64 = 16;
const TOUCH_DEBOUNCE_UP_NO_MOVE_MS: u64 = 32;
pub(super) const TOUCH_DEBOUNCE_UP_NO_MOVE_EARLY_MS: u64 = 112;
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
pub(crate) struct TouchPoint {
    pub(crate) x: u16,
    pub(crate) y: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TouchSample {
    pub(crate) touch_count: u8,
    pub(crate) points: [TouchPoint; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchSwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchEventKind {
    Down,
    Move,
    Up,
    Tap,
    LongPress,
    Swipe(TouchSwipeDirection),
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TouchEvent {
    pub(crate) kind: TouchEventKind,
    pub(crate) t_ms: u64,
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) contact_x: u16,
    pub(crate) contact_y: u16,
    pub(crate) start_x: u16,
    pub(crate) start_y: u16,
    pub(crate) duration_ms: u16,
    pub(crate) touch_count: u8,
    pub(crate) move_count: u16,
    pub(crate) max_travel_px: u16,
    pub(crate) release_debounce_ms: u16,
    pub(crate) dropout_count: u16,
}

#[derive(Clone, Copy, Debug)]
enum TouchHsmEvent {
    Sample {
        now_ms: u64,
        sample: TouchSample,
    },
    /// Ends whatever interaction is in flight before its raw contact is
    /// resolved by a sample, without waiting for the controller to report a
    /// release. The idle state treats this as a no-op (nothing to cancel);
    /// every other state emits `Cancel` and returns to idle, the same shape
    /// as `ButtonHsm::cancel` for the button recognizer.
    Cancel {
        now_ms: u64,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TouchEngineOutput {
    pub(crate) events: [Option<TouchEvent>; 3],
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

pub(crate) struct TouchEngine {
    machine: statig::blocking::StateMachine<TouchHsm>,
}

impl Default for TouchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TouchEngine {
    pub(crate) fn new() -> Self {
        Self {
            machine: TouchHsm::new().state_machine(),
        }
    }

    pub(crate) fn is_idle(&self) -> bool {
        matches!(self.machine.state(), hsm::State::Idle {})
    }

    pub(crate) fn tick(&mut self, now_ms: u64, sample: TouchSample) -> TouchEngineOutput {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&TouchHsmEvent::Sample { now_ms, sample }, &mut context);
        context.finish()
    }

    /// Ends an in-flight interaction before its raw contact is released or
    /// replaced -- the touch counterpart of `ButtonRecognizer::cancel`. A
    /// no-op (no emitted event) if nothing is in flight.
    pub(crate) fn cancel(&mut self, now_ms: u64) -> TouchEngineOutput {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&TouchHsmEvent::Cancel { now_ms }, &mut context);
        context.finish()
    }
}
