mod backend;
pub(in crate::firmware::ui) mod io;
mod screen_update_state;

pub(crate) use super::screen::ambient_view::AmbientHomeAction;
pub(crate) use super::screen::analog_clock::{
    resolve_clock_publish, ClockPoll, ClockPublishDecision, EstimatedSettle, MinuteIntent,
    SettleOutcome, TargetMinute,
};
pub(crate) use backend::{Backend, FrontlightCalibrationEffect, InitError, UiCycleStepError};
pub(crate) use io::{take_gesture, LvglGestureEvent, LvglGestureKind, LvglGestureState};
pub(crate) use render::DirtyArea;
pub(crate) use screen_update_state::{select_fast_update_operation, FastUpdateOperation};

// Panel geometry belongs to the board, not the UI layer. Sourced from the
// driver that owns it rather than restated here.
const WIDTH: i32 = inkplate_tempera::E_INK_WIDTH as i32;
const HEIGHT: i32 = inkplate_tempera::E_INK_HEIGHT as i32;

#[no_mangle]
pub(crate) extern "C" fn meditamer_lvgl_alloc_pool(size: usize) -> *mut core::ffi::c_void {
    backend::alloc_pool(size)
}
