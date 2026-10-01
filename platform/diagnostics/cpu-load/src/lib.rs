//! Per-core elapsed-time accounting, with optional ESP32 scheduler/IRQ hooks.
#![no_std]

mod history;
mod model;
pub use history::{History, Snapshot};
pub use model::{Core, Event, Reading};

#[cfg(feature = "esp32")]
mod esp32;
#[cfg(feature = "esp32")]
pub use esp32::touch_timer_irq_snapshot;
#[cfg(feature = "esp32")]
pub use esp32::{execution_context, pause_for_flash, sample, take_task_pointer_trace};

pub mod profile;
#[cfg(feature = "esp32")]
pub use esp32::{
    profile_snapshot, reset_profile_timings, tail_begin, tail_end, tail_note_pipeline,
    tail_note_pipeline_phase, tail_note_touch_phase,
};
