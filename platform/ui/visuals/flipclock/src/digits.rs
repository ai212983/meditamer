//! Digit cards, rasterized at build time from the repository's IBM Plex face.
//!
//! Baked into flash rather than loaded from the SD card, for two reasons that
//! are worth recording because "assets on SD" is the obvious first instinct:
//! the board's TF slot is wired as three-wire 1-bit SD (no chip select broken
//! out, so no SPI-mode driver can reach it) and `esp-hal` ships no SDMMC host
//! driver, so reading a file would mean writing one first. And it would buy
//! nothing -- ten cards is about 13 KB against a 16 MiB part.
//!
//! `CARD_W`, `CARD_H`, `CARD_STRIDE`, `CARD_BYTES` and `DIGIT_CARDS` all come
//! from the generated file, which is the single source of truth for card
//! geometry: a build script cannot import the crate it is building, so the
//! constants travel outward from `build.rs` rather than being duplicated here.

use super::flap::Bitmap;

include!(concat!(env!("OUT_DIR"), "/flipclock_digits.rs"));

/// Half a card's height, and the row the hinge falls on.
pub const HALF_H: i32 = CARD_H / 2;

/// The whole card for `digit`, which must be 0..=9.
pub fn card(digit: u8) -> Bitmap<'static> {
    Bitmap {
        data: &DIGIT_CARDS[digit as usize],
        width: CARD_W,
        height: CARD_H,
    }
}

/// One half of a card. `bottom` selects the half below the hinge.
///
/// The two halves are what the renderer actually consumes: the static faces the
/// housing holds, and whichever one is currently in motion.
pub fn half(digit: u8, bottom: bool) -> Bitmap<'static> {
    let start = if bottom {
        HALF_H as usize * CARD_STRIDE
    } else {
        0
    };
    Bitmap {
        data: &DIGIT_CARDS[digit as usize][start..start + HALF_H as usize * CARD_STRIDE],
        width: CARD_W,
        height: HALF_H,
    }
}
