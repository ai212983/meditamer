use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};

use lightvgl_sys as lv;

use super::dither::{self, DirtyArea};
use super::{HEIGHT, WIDTH};
use crate::firmware::{
    touch::types::{TouchEvent, TouchEventKind},
    types::InkplateDriver,
};

static ACTIVE_DISPLAY: AtomicPtr<InkplateDriver> = AtomicPtr::new(ptr::null_mut());
static INPUT_X: AtomicI32 = AtomicI32::new(0);
static INPUT_Y: AtomicI32 = AtomicI32::new(0);
static INPUT_PRESSED: AtomicBool = AtomicBool::new(false);
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
    _input: *mut lv::lv_indev_t,
    data: *mut lv::lv_indev_data_t,
) {
    if let Some(data) = unsafe { data.as_mut() } {
        data.point.x = INPUT_X.load(Ordering::Relaxed);
        data.point.y = INPUT_Y.load(Ordering::Relaxed);
        data.state = if INPUT_PRESSED.load(Ordering::Acquire) {
            lv::lv_indev_state_t_LV_INDEV_STATE_PRESSED
        } else {
            lv::lv_indev_state_t_LV_INDEV_STATE_RELEASED
        };
    }
}
