//! Medinote product library root.
//!
//! Product-owned app state, input policy, observations, presentation, and
//! checked UI construction. `targets/medinote-waveshare` composes this crate
//! with `boards/waveshare-rlcd42` hardware.

#![no_std]

pub mod apps;
pub mod catalogue;
pub mod config;
pub mod controls;
pub mod input;
pub mod observations;
pub mod power;
pub mod presentation;

#[cfg(feature = "lvgl")]
pub mod ui;

/// The product name `targets/medinote-waveshare` composes against.
pub const fn product_name() -> &'static str {
    "medinote"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_name_identifies_medinote() {
        assert_eq!(product_name(), "medinote");
    }
}
