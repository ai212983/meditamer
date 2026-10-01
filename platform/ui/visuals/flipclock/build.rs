//! Generates `flipclock_digits.rs` for `flipclock::digits`.
//!
//! Follows `products/meditamer/build.rs`: the rasterizer is a host-only crate
//! under `tools/`, this script only decides the spec and writes the result into
//! `OUT_DIR`. `assets/fonts` stays at the repo root for the same reason it does
//! there -- more than one consumer reaches the same file, and duplicating it
//! would let the copies drift.

use std::{env, fs, path::PathBuf};

use flip_digit_compiler::{generate, CardSpec};

/// Card geometry. Both dimensions are chosen against the ST7305's partial
/// window granularity -- 8 px in logical X, 12 px in logical Y -- so one card's
/// dirty box never drags a neighbouring column group in with it.
const CARD_W: i32 = 80;
const CARD_H: i32 = 120;
/// Fills about 60 percent of the card height, which is roughly what a physical
/// split-flap leaves around its numerals.
const SIZE_PX: f32 = 100.0;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let source = manifest.join("../../../../assets/fonts/IBMPlexSans-Variable.ttf");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed=build.rs");

    let generated = generate(&CardSpec {
        source: &source,
        card_w: CARD_W,
        card_h: CARD_H,
        size_px: SIZE_PX,
        characters: "0123456789",
    })
    .expect("rasterize flip-clock digit cards");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("flipclock_digits.rs"), generated).expect("write digit cards");
}
