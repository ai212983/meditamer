mod backlight;
mod bootstrap;
pub(crate) mod diagnostics;
pub(crate) mod display_task;
mod face_down;
mod serial_task;
pub(crate) mod service_mode;

pub(crate) use backlight::{run_backlight_timeline, trigger_backlight_cycle};
pub use bootstrap::run;
pub(crate) use face_down::{update_face_down_toggle, FaceDownToggleState};
