#![no_std]
#![allow(dead_code)]

pub use shell;

// Pure config/time-math/geometry for the Ambient Home prototype
// (docs/archive/features/ambient-home-prototype.md). No LVGL dependency, so it is
// pulled in unmodified; its own `#[cfg(all(test, not(target_os = "none")))]`
// unit tests run here on host.
#[path = "../../../../products/meditamer/src/firmware/ui/screen/ambient_view/model.rs"]
pub mod ambient_home_model;

// Pure bit-plane composer for the Ambient Home sky/sun surface
// (products/meditamer/src/firmware/ui/screen/ambient_view/composer.rs).
// Core-only like the model above; its unit tests run here on host.
#[path = "../../../../products/meditamer/src/firmware/ui/screen/ambient_view/composer.rs"]
pub mod ambient_composer;

// The 128 px Ambient Home clock face. The module body is generated, and this
// crate's `build.rs` writes the same table the firmware build does, so the
// module compiles here against the harness's LVGL exactly as it does in the
// firmware.
#[path = "../../../../products/meditamer/src/firmware/ui/overlay/clock_font.rs"]
pub mod ambient_clock_font;

// Pure `HH:MM` formatting and monotonic timeout behavior for the shell-owned
// clock overlay.
#[path = "../../../../products/meditamer/src/firmware/ui/overlay/clock_model.rs"]
pub mod clock_overlay_model;

// The 64 px Ambient Home environmental face, generated from the same source
// and glyph subset as the firmware build.
#[path = "../../../../products/meditamer/src/firmware/ui/screen/ambient_view/environment_font.rs"]
pub mod ambient_environment_font;

// Pure minute-cadence policy for the runtime analog-clock ambient screen
// (products/meditamer/src/firmware/ui/screen/analog_clock/model.rs). No LVGL
// and no allocator dependency, so it is pulled in unmodified; its own
// `#[cfg(test)]` unit tests run here on host alongside the cadence
// integration tests.
#[path = "../../../../products/meditamer/src/firmware/ui/screen/analog_clock/model.rs"]
pub mod analog_clock_model;

// Pure input-transition presentation policy for the LVGL backend
// (products/meditamer/src/firmware/ui/lvgl/backend/transition_readiness.rs).
// No LVGL dependency, pulled in unmodified like the clock model above.
#[path = "../../../../products/meditamer/src/firmware/ui/lvgl/backend/transition_readiness.rs"]
pub mod input_transition_readiness;

#[cfg(test)]
mod lvgl_overlay_semantics;
