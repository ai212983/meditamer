use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};
use core::{cell::RefCell, ptr};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, blocking_mutex::Mutex};
use heapless::Deque;
use lightvgl_sys as lv;

use render::dither::{self, DirtyArea};
use super::{HEIGHT, WIDTH};
use crate::firmware::{
    touch::lvgl_multitouch::LvglContactBatch,
    touch::types::{TouchEvent, TouchEventKind},
    types::InkplateDriver,
};

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

static ACTIVE_DISPLAY: AtomicPtr<InkplateDriver> = AtomicPtr::new(ptr::null_mut());
static GESTURE_INPUT: AtomicPtr<lv::lv_indev_t> = AtomicPtr::new(ptr::null_mut());
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

pub(super) fn register_gesture_input(input: *mut lv::lv_indev_t) {
    GESTURE_INPUT.store(input, Ordering::Release);
}

pub(crate) fn take_gesture() -> Option<LvglGestureEvent> {
    GESTURE_EVENTS.lock(|events| events.borrow_mut().pop_front())
}

pub(super) fn begin(display: &mut InkplateDriver) {
    reset_dirty_area();
    ACTIVE_DISPLAY.store(display, Ordering::Release);
}

pub(super) fn finish() -> Option<DirtyArea> {
    ACTIVE_DISPLAY.store(ptr::null_mut(), Ordering::Release);
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

pub(super) unsafe extern "C" fn flush_callback(
    lv_display: *mut lv::lv_display_t,
    area: *const lv::lv_area_t,
    pixels: *mut u8,
) {
    let display = ACTIVE_DISPLAY.load(Ordering::Acquire);
    if !display.is_null() && !area.is_null() {
        let area = unsafe { *area };
        let area = DirtyArea {
            x1: area.x1,
            y1: area.y1,
            x2: area.x2,
            y2: area.y2,
        };
        let copied = unsafe { dither::blit_l8(area, pixels, (&mut *display).framebuffer_bw_mut()) };
        if copied {
            record_dirty_area(area);
        }
    }
    unsafe { lv::lv_display_flush_ready(lv_display) };
}

pub(super) unsafe extern "C" fn input_callback(
    input: *mut lv::lv_indev_t,
    data: *mut lv::lv_indev_data_t,
) {
    if let Some(data) = unsafe { data.as_mut() } {
        let batch = MULTITOUCH_BATCH.lock(|pending| pending.borrow_mut().take());
        if let Some(batch) = batch {
            let empty_touch = lv::lv_indev_touch_data_t {
                point: lv::lv_point_t { x: 0, y: 0 },
                state: lv::lv_indev_state_t_LV_INDEV_STATE_RELEASED,
                id: 0,
                timestamp: 0,
            };
            let mut touches = [empty_touch; 4];
            let mut touch_count = 0usize;
            let mut primary = None;
            for update in batch.updates.into_iter().flatten() {
                touches[touch_count] = lv::lv_indev_touch_data_t {
                    point: lv::lv_point_t {
                        x: i32::from(update.point.x),
                        y: i32::from(update.point.y),
                    },
                    state: if update.pressed {
                        lv::lv_indev_state_t_LV_INDEV_STATE_PRESSED
                    } else {
                        lv::lv_indev_state_t_LV_INDEV_STATE_RELEASED
                    },
                    id: update.id,
                    timestamp: update.timestamp,
                };
                if primary.is_none() && update.pressed {
                    primary = Some(update.point);
                }
                touch_count += 1;
            }

            unsafe {
                lv::lv_indev_gesture_recognizers_update(
                    input,
                    touches.as_mut_ptr(),
                    touch_count as u16,
                );
                lv::lv_indev_gesture_recognizers_set_data(input, data);
            }
            if let Some(primary) = primary {
                data.point.x = i32::from(primary.x);
                data.point.y = i32::from(primary.y);
            }
            return;
        }

        data.point.x = INPUT_X.load(Ordering::Relaxed);
        data.point.y = INPUT_Y.load(Ordering::Relaxed);
        data.state = if INPUT_PRESSED.load(Ordering::Acquire) {
            lv::lv_indev_state_t_LV_INDEV_STATE_PRESSED
        } else {
            lv::lv_indev_state_t_LV_INDEV_STATE_RELEASED
        };
    }
}

pub(super) unsafe extern "C" fn gesture_callback(event: *mut lv::lv_event_t) {
    if event.is_null() {
        return;
    }
    // LVGL uses LV_EVENT_GESTURE for both multi-touch recognizer events and
    // legacy object gestures. Only recognizer events carry the input device as
    // both the callback target and event parameter. The gesture accessors cast
    // the parameter to lv_indev_t and do not validate the gesture-type index.
    let registered_input = GESTURE_INPUT.load(Ordering::Acquire);
    let event_param = unsafe { lv::lv_event_get_param(event) };
    let current_target = unsafe { lv::lv_event_get_current_target(event) };
    if registered_input.is_null()
        || event_param != registered_input.cast()
        || current_target != registered_input.cast()
    {
        return;
    }

    let gesture_type = unsafe { lv::lv_event_get_gesture_type(event) };
    if !matches!(
        gesture_type,
        lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_PINCH
            | lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_ROTATE
            | lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_TWO_FINGERS_SWIPE
    ) {
        return;
    }
    // Recognizers emit every motion sample after recognition. The e-paper UI
    // only consumes the completed value, so do not flood the cross-callback
    // queue or repeatedly dereference recognizer state during an active swipe.
    if unsafe { lv::lv_event_get_gesture_state(event, gesture_type) }
        != lv::lv_indev_gesture_state_t_LV_INDEV_GESTURE_STATE_ENDED
    {
        return;
    }
    let state = LvglGestureState::Ended;
    let kind = match gesture_type {
        lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_PINCH => LvglGestureKind::Pinch {
            scale: unsafe { lv::lv_event_get_pinch_scale(event) },
        },
        lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_ROTATE => LvglGestureKind::Rotation {
            radians: unsafe { lv::lv_event_get_rotation(event) },
        },
        lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_TWO_FINGERS_SWIPE => {
            LvglGestureKind::TwoFingerSwipe {
                direction: map_direction(unsafe { lv::lv_event_get_two_fingers_swipe_dir(event) }),
                distance_px: unsafe { lv::lv_event_get_two_fingers_swipe_distance(event) },
            }
        }
        _ => return,
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

fn map_direction(direction: lv::lv_dir_t) -> LvglGestureDirection {
    match direction {
        lv::lv_dir_t_LV_DIR_LEFT => LvglGestureDirection::Left,
        lv::lv_dir_t_LV_DIR_RIGHT => LvglGestureDirection::Right,
        lv::lv_dir_t_LV_DIR_TOP => LvglGestureDirection::Up,
        lv::lv_dir_t_LV_DIR_BOTTOM => LvglGestureDirection::Down,
        _ => LvglGestureDirection::Unknown,
    }
}
