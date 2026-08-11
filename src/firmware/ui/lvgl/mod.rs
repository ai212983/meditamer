mod ambient_picker;
mod backend;
mod base_overlays;
mod carousel;
mod catalogue_presenter;
mod dither;
mod gesture_test;
mod home;
mod intent_bridge;
mod io;
mod launcher;
mod overlay_settings;
#[cfg(feature = "ui-provider-fixture")]
mod provider_fixture;

pub(crate) use backend::{Backend, InitError, UiCycleStepError};
pub(crate) use dither::DirtyArea;
pub(crate) use intent_bridge::take_full_repaint_request;
pub(crate) use io::{take_gesture, LvglGestureEvent, LvglGestureKind, LvglGestureState};

const WIDTH: i32 = 600;
const HEIGHT: i32 = 600;

#[no_mangle]
pub extern "C" fn meditamer_lvgl_alloc_pool(size: usize) -> *mut core::ffi::c_void {
    backend::alloc_pool(size)
}
