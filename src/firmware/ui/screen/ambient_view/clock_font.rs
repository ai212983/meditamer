//! The Ambient Home clock face: IBM Plex Sans at 128 px, restricted to the
//! digits, colon, and dash that `HH:MM` and its `--:--` placeholder need.
//!
//! LVGL's built-in Montserrat tables stop at 48 px, so `build.rs` compiles this
//! one from `assets/fonts/IBMPlexSans-Variable.ttf` with
//! `tools/lvgl_font_compiler` and writes it into `OUT_DIR`.

include!(concat!(env!("OUT_DIR"), "/ambient_clock_font.rs"));
