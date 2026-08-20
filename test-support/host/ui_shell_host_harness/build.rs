use std::{env, fs, path::PathBuf};

use lvgl_font_compiler::generate_ambient_clock_font;

/// Compiles the same Ambient Home clock face the firmware ships, so
/// `src/firmware/ui/screen/ambient_view/clock_font.rs` can be pulled in here
/// unmodified and rendered through LVGL on the host.
fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let assets = manifest_dir.join("../../../assets/fonts");
    println!("cargo:rerun-if-changed={}", assets.display());

    let generated = generate_ambient_clock_font(&assets)
        .unwrap_or_else(|error| panic!("ambient clock font compile failed: {error}"));

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let path = out_dir.join("ambient_clock_font.rs");
    fs::write(&path, generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}
