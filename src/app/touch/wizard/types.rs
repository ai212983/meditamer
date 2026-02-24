use core::fmt::Write;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
};
use heapless::String;
use u8g2_fonts::types::{FontColor, HorizontalAlignment, VerticalPosition};

use crate::app::{
    config::{META_FONT, SCREEN_HEIGHT, SCREEN_WIDTH, TITLE_FONT},
    types::InkplateDriver,
};

use super::{
    config::{TOUCH_WIZARD_SESSION_EVENTS, TOUCH_WIZARD_SWIPE_TRACE_SAMPLES},
    types::{
        TouchEvent, TouchEventKind, TouchSwipeDirection, TouchWizardSessionEvent,
        TouchWizardSwipeTraceSample,
    },
};

const TARGET_RADIUS_PX: i32 = 26;
const TARGET_HIT_RADIUS_PX: i32 = TARGET_RADIUS_PX;
const SWIPE_TRACE_MAX_POINTS: usize = 32;
const CONTINUE_BUTTON_WIDTH: i32 = 192;
const CONTINUE_BUTTON_HEIGHT: i32 = 52;
const SWIPE_MARK_BUTTON_WIDTH: i32 = 232;
const SWIPE_MARK_BUTTON_HEIGHT: i32 = 44;
const SWIPE_CASE_COUNT: u8 = 8;
const SWIPE_CASE_START_RADIUS_PX: i32 = 60;
const SWIPE_CASE_END_RADIUS_PX: i32 = 72;
const TRACE_DIRECTION_UNKNOWN: u8 = 0xFF;
const TRACE_SPEED_UNKNOWN: u8 = 0xFF;
const TRACE_VERDICT_PASS: u8 = 0;
const TRACE_VERDICT_MISMATCH: u8 = 1;
const TRACE_VERDICT_RELEASE_NO_SWIPE: u8 = 2;
const TRACE_VERDICT_MANUAL_MARK: u8 = 3;
const TRACE_VERDICT_SKIP: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WizardPhase {
    Intro,
    TapCenter,
    TapTopLeft,
    TapBottomRight,
    SwipeRight,
    Complete,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WizardDispatch {
    Inactive,
    Consumed,
    Finished,
}

pub(crate) struct TouchCalibrationWizard {
    phase: WizardPhase,
    hint: &'static str,
    last_tap: Option<TapAttempt>,
    swipe_trace: SwipeTrace,
    last_swipe: Option<SwipeAttempt>,
    swipe_trace_pending_points: u8,
    swipe_debug: SwipeDebugStats,
    swipe_case_index: u8,
    swipe_case_passed: u8,
    swipe_case_failed: u16,
    swipe_case_attempts: u16,
    manual_swipe_marks: u16,
    pending_swipe_release: Option<PendingSwipeRelease>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TapAttempt {
    x: i32,
    y: i32,
    hit: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SwipePoint {
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwipeTrace {
    points: [SwipePoint; SWIPE_TRACE_MAX_POINTS],
    len: u8,
}

impl Default for SwipeTrace {
    fn default() -> Self {
        Self {
            points: [SwipePoint::default(); SWIPE_TRACE_MAX_POINTS],
            len: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwipeAttempt {
    start: SwipePoint,
    end: SwipePoint,
    accepted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingSwipeRelease {
    t_ms: u64,
    start: SwipePoint,
    end: SwipePoint,
    duration_ms: u16,
    move_count: u16,
    max_travel_px: u16,
    release_debounce_ms: u16,
    dropout_count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwipeSpeedTier {
    ExtraFast,
    Fast,
    Medium,
    Slow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwipeCaseSpec {
    direction: TouchSwipeDirection,
    speed: SwipeSpeedTier,
    start: SwipePoint,
    end: SwipePoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwipeDebugKind {
    None,
    Down,
    Move,
    Up,
    Swipe(TouchSwipeDirection),
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwipeDebugStats {
    down_count: u16,
    move_count: u16,
    up_count: u16,
    swipe_count: u16,
    cancel_count: u16,
    last_kind: SwipeDebugKind,
    last_start: SwipePoint,
    last_end: SwipePoint,
    last_duration_ms: u16,
    last_move_count: u16,
    last_max_travel_px: u16,
    last_release_debounce_ms: u16,
    last_dropout_count: u16,
}

impl Default for SwipeDebugStats {
    fn default() -> Self {
        Self {
            down_count: 0,
            move_count: 0,
            up_count: 0,
            swipe_count: 0,
            cancel_count: 0,
            last_kind: SwipeDebugKind::None,
            last_start: SwipePoint::default(),
            last_end: SwipePoint::default(),
            last_duration_ms: 0,
            last_move_count: 0,
            last_max_travel_px: 0,
            last_release_debounce_ms: 0,
            last_dropout_count: 0,
        }
    }
}

