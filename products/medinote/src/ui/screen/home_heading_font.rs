//! Home's heading face ("Medinote", relabeled "Sleep" on wake): Ark Pixel,
//! 16px proportional Latin build (`assets/fonts/ArkPixel-OFL.txt`).
//!
//! `build.rs` compiles this from `assets/fonts/ArkPixel-16px-Proportional-
//! Latin.ttf` with `tools/lvgl_font_compiler` and writes it into `OUT_DIR`.

include!(concat!(env!("OUT_DIR"), "/home_heading_font.rs"));
