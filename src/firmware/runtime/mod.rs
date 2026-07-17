mod backlight;
mod bootstrap;
pub(crate) mod diagnostics;
pub(crate) mod display_task;
mod periodic;
pub(crate) mod scheduling;
mod serial_task;
pub(crate) mod service_mode;

pub(crate) use backlight::{run_backlight_timeline, trigger_backlight_cycle};
pub use bootstrap::run;
