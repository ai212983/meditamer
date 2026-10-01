//! The clock-overlay face: IBM Plex Sans at 128 px, restricted to the digits
//! and colon used by `HH:MM`.
//!
//! LVGL's built-in Montserrat tables stop at 48 px, so `build.rs` compiles this
//! one from `assets/fonts/IBMPlexSans-Variable.ttf` with
//! `tools/lvgl_font_compiler` and writes it into `OUT_DIR`.

include!(concat!(env!("OUT_DIR"), "/ambient_clock_font.rs"));
