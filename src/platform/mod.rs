pub mod gpio_fast;
pub mod inkplate;
// The outer `platform` directory (renamed from `drivers`, S4) and this
// `platform.rs` submodule are unrelated by name coincidence; the plan
// preserves the submodule's identity and relative position as-is.
#[allow(clippy::module_inception)]
pub mod platform;
