//! Home's caption face ("Home", relabeled "Awake" on wake): Ark Pixel, 12px
//! proportional Latin build (`assets/fonts/ArkPixel-OFL.txt`).
//!
//! `build.rs` compiles this from `assets/fonts/ArkPixel-12px-Proportional-
//! Latin.ttf` with `tools/lvgl_font_compiler` and writes it into `OUT_DIR`.

include!(concat!(env!("OUT_DIR"), "/home_caption_font.rs"));
