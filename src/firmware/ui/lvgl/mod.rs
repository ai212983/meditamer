mod backend;
mod dither;
pub(in crate::firmware::ui) mod intent_bridge;
pub(in crate::firmware::ui) mod io;

pub(crate) use super::screen::ambient_view::AmbientHomeAction;
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
