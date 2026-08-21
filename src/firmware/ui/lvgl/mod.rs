mod backend;
pub(in crate::firmware::ui) mod io;

pub(crate) use super::screen::ambient_view::AmbientHomeAction;
pub(crate) use backend::{Backend, InitError, UiCycleStepError};
pub(crate) use io::{take_gesture, LvglGestureEvent, LvglGestureKind, LvglGestureState};
pub(crate) use render::intent_bridge::take_full_repaint_request;
pub(crate) use render::DirtyArea;

// Panel geometry belongs to the board, not the UI layer. Sourced from the
// driver that owns it rather than restated here; it moves to the board's own
// crate when `boards/` exists (ADR-0015).
const WIDTH: i32 = crate::platform::inkplate::E_INK_WIDTH as i32;
const HEIGHT: i32 = crate::platform::inkplate::E_INK_HEIGHT as i32;

#[no_mangle]
pub extern "C" fn meditamer_lvgl_alloc_pool(size: usize) -> *mut core::ffi::c_void {
    backend::alloc_pool(size)
}
