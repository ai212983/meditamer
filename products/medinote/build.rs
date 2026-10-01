//! Compiles Medinote Home's heading/caption faces (`assets/fonts/`, Ark
//! Pixel) into LVGL font tables for `src/ui/screen/home.rs`, writing them
//! into `OUT_DIR` for `home_heading_font.rs`/`home_caption_font.rs` to
//! `include!`.
//!
//! Runs unconditionally, same shape as `products/meditamer/build.rs`'s own
//! font compilation: the `ui` module that consumes the generated tables is
//! gated behind the `lvgl` feature, so this only costs a plain
//! `cargo build -p medinote` a TTF parse it then does nothing with.

use std::{env, fs, path::PathBuf};

use lvgl_font_compiler::{
    generate_medinote_home_caption_font_for_render, generate_medinote_home_heading_font_for_render,
};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .join("../..")
        .canonicalize()
        .expect("products/medinote is two levels below the repo root");
    let assets = repo_root.join("assets/fonts");
    println!("cargo:rerun-if-changed={}", assets.display());

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));

    let generated = generate_medinote_home_heading_font_for_render(&assets)
        .unwrap_or_else(|error| panic!("home heading font compile failed: {error}"));
    let path = out_dir.join("home_heading_font.rs");
    fs::write(&path, generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));

    let generated = generate_medinote_home_caption_font_for_render(&assets)
        .unwrap_or_else(|error| panic!("home caption font compile failed: {error}"));
    let path = out_dir.join("home_caption_font.rs");
    fs::write(&path, generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}
