#![forbid(unsafe_code)]

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, blocking_mutex::Mutex};
use heapless::Deque;
use render::lvgl_adapter::{
    GestureDirection, GestureEvent, PointerContact, PointerSample, PointerState,
};

use super::{HEIGHT, WIDTH};
use crate::firmware::{
    touch::lvgl_multitouch::LvglContactBatch,
    touch::types::{TouchEvent, TouchEventKind},
};
use inkplate_tempera::panel_blit;
use render::DirtyArea;

const GESTURE_QUEUE_CAPACITY: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LvglGestureKind {
    Pinch {
        scale: f32,
    },
    Rotation {
        radians: f32,
    },
    TwoFingerSwipe {
        direction: LvglGestureDirection,
        distance_px: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LvglGestureDirection {
    Left,
    Right,
    Up,
    Down,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LvglGestureState {
    Ended,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LvglGestureEvent {
    pub(crate) kind: LvglGestureKind,
    pub(crate) state: LvglGestureState,
}

static INPUT_X: AtomicI32 = AtomicI32::new(0);
static INPUT_Y: AtomicI32 = AtomicI32::new(0);
static INPUT_PRESSED: AtomicBool = AtomicBool::new(false);
static MULTITOUCH_BATCH: Mutex<CriticalSectionRawMutex, RefCell<Option<LvglContactBatch>>> =
    Mutex::new(RefCell::new(None));
static GESTURE_EVENTS: Mutex<
    CriticalSectionRawMutex,
    RefCell<Deque<LvglGestureEvent, GESTURE_QUEUE_CAPACITY>>,
> = Mutex::new(RefCell::new(Deque::new()));
static DIRTY_X1: AtomicI32 = AtomicI32::new(WIDTH);
static DIRTY_Y1: AtomicI32 = AtomicI32::new(HEIGHT);
static DIRTY_X2: AtomicI32 = AtomicI32::new(-1);
static DIRTY_Y2: AtomicI32 = AtomicI32::new(-1);
pub(super) fn retire_input() {
    INPUT_PRESSED.store(false, Ordering::Release);
    MULTITOUCH_BATCH.lock(|pending| *pending.borrow_mut() = None);
    GESTURE_EVENTS.lock(|events| events.borrow_mut().clear());
}

pub(super) fn update_touch(event: TouchEvent) {
    match event.kind {
        TouchEventKind::Down | TouchEventKind::Move | TouchEventKind::LongPress => {
            INPUT_X.store(i32::from(event.x), Ordering::Relaxed);
            INPUT_Y.store(i32::from(event.y), Ordering::Relaxed);
            INPUT_PRESSED.store(true, Ordering::Release);
        }
        TouchEventKind::Up | TouchEventKind::Cancel => {
            INPUT_X.store(i32::from(event.x), Ordering::Relaxed);
            INPUT_Y.store(i32::from(event.y), Ordering::Relaxed);
            INPUT_PRESSED.store(false, Ordering::Release);
        }
        TouchEventKind::Tap | TouchEventKind::Swipe(_) => {}
    }
}

pub(super) fn queue_multitouch(batch: LvglContactBatch) {
    MULTITOUCH_BATCH.lock(|pending| {
        *pending.borrow_mut() = Some(batch);
    });
}

pub(crate) fn take_gesture() -> Option<LvglGestureEvent> {
    GESTURE_EVENTS.lock(|events| events.borrow_mut().pop_front())
}

pub(super) fn begin() {
    reset_dirty_area();
}

pub(super) fn finish() -> Option<DirtyArea> {
    take_dirty_area()
}

fn reset_dirty_area() {
    DIRTY_X1.store(WIDTH, Ordering::Relaxed);
    DIRTY_Y1.store(HEIGHT, Ordering::Relaxed);
    DIRTY_X2.store(-1, Ordering::Relaxed);
    DIRTY_Y2.store(-1, Ordering::Relaxed);
}

fn record_dirty_area(area: DirtyArea) {
    let current = dirty_area();
    let dirty = current.map_or(area, |current| current.union(area));
    DIRTY_X1.store(dirty.x1, Ordering::Relaxed);
    DIRTY_Y1.store(dirty.y1, Ordering::Relaxed);
    DIRTY_X2.store(dirty.x2, Ordering::Relaxed);
    DIRTY_Y2.store(dirty.y2, Ordering::Relaxed);
}

fn dirty_area() -> Option<DirtyArea> {
    let area = DirtyArea {
        x1: DIRTY_X1.load(Ordering::Relaxed),
        y1: DIRTY_Y1.load(Ordering::Relaxed),
        x2: DIRTY_X2.load(Ordering::Relaxed),
        y2: DIRTY_Y2.load(Ordering::Relaxed),
    };
    (area.x1 <= area.x2 && area.y1 <= area.y2).then_some(area)
}

fn take_dirty_area() -> Option<DirtyArea> {
    let area = dirty_area();
    reset_dirty_area();
    area
}

pub(super) fn blit_flush(area: DirtyArea, pixels: &[u8], framebuffer: &mut [u8]) -> bool {
    let copied = panel_blit::blit_l8(area, pixels, framebuffer);
    if copied {
        record_dirty_area(area);
    }
    copied
}

pub(super) fn pointer_sample() -> PointerSample {
    let batch = MULTITOUCH_BATCH.lock(|pending| pending.borrow_mut().take());
    if let Some(batch) = batch {
        return PointerSample::multi(batch.updates.map(|update| {
            update.map(|update| PointerContact {
                point: (i32::from(update.point.x), i32::from(update.point.y)),
                state: if update.pressed {
                    PointerState::Pressed
                } else {
                    PointerState::Released
                },
                id: update.id,
                timestamp: update.timestamp,
            })
        }));
    }
    PointerSample::single(
        INPUT_X.load(Ordering::Relaxed),
        INPUT_Y.load(Ordering::Relaxed),
        if INPUT_PRESSED.load(Ordering::Acquire) {
            PointerState::Pressed
        } else {
            PointerState::Released
        },
    )
}

pub(super) fn record_gesture(event: GestureEvent) {
    let state = LvglGestureState::Ended;
    let kind = match event {
        GestureEvent::Pinch { scale } => LvglGestureKind::Pinch { scale },
        GestureEvent::Rotation { radians } => LvglGestureKind::Rotation { radians },
        GestureEvent::TwoFingerSwipe {
            direction,
            distance_px,
        } => LvglGestureKind::TwoFingerSwipe {
            direction: map_direction(direction),
            distance_px,
        },
    };
    let gesture = LvglGestureEvent { kind, state };
    GESTURE_EVENTS.lock(|events| {
        let mut events = events.borrow_mut();
        if events.is_full() {
            let _ = events.pop_front();
        }
        let _ = events.push_back(gesture);
    });
}

fn map_direction(direction: GestureDirection) -> LvglGestureDirection {
    match direction {
        GestureDirection::Left => LvglGestureDirection::Left,
        GestureDirection::Right => LvglGestureDirection::Right,
        GestureDirection::Up => LvglGestureDirection::Up,
        GestureDirection::Down => LvglGestureDirection::Down,
        GestureDirection::Unknown => LvglGestureDirection::Unknown,
    }
}
