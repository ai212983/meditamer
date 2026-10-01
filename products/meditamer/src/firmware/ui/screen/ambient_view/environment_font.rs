//! The Ambient Home environmental-reading face: IBM Plex Sans at 64 px,
//! restricted to the characters used by temperature, humidity, and their
//! unavailable placeholder.
//!
//! `build.rs` compiles it from `assets/fonts/IBMPlexSans-Variable.ttf` with
//! `tools/lvgl_font_compiler` and writes it into `OUT_DIR`.

include!(concat!(env!("OUT_DIR"), "/ambient_environment_font.rs"));
