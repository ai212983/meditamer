#![no_std]
#![allow(dead_code)]

pub use shell;

// Pure config/time-math/geometry for the Ambient Home prototype
// (docs/plans/ambient-home-prototype.md). No LVGL dependency, so it is
// pulled in unmodified; its own `#[cfg(all(test, not(target_os = "none")))]`
// unit tests run here on host.
#[path = "../../../../src/firmware/ui/screen/ambient_view/model.rs"]
pub mod ambient_home_model;

// The 128 px Ambient Home clock face. The module body is generated, and this
// crate's `build.rs` writes the same table the firmware build does, so the
// module compiles here against the harness's LVGL exactly as it does in the
// firmware.
#[path = "../../../../src/firmware/ui/screen/ambient_view/clock_font.rs"]
pub mod ambient_clock_font;

#[cfg(test)]
mod lvgl_overlay_semantics;
