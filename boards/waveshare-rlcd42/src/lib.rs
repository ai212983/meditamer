//! Waveshare ESP32-S3-RLCD-4.2 board support: ST7305 panel and waveform
//! driver, USB-Serial-JTAG RX primitive, and the LVGL-to-panel flush bridge
//! (product and target axis completion plan, Phase 2).
//!
//! Product state, screen construction, cadence policy, and presentation live
//! in `products/medinote`; chip startup, task wiring, and peripheral
//! composition live in `targets/medinote-waveshare`. This crate owns neither
//! -- it is the board-extraction counterpart to `boards/inkplate-tempera`,
//! closing the asymmetry that plan's Phase 1 left open (that crate already
//! had no product content; this one did, and Phase 2 is what moved it out).

#![no_std]

pub mod buttons;
pub mod jtag_rx;
pub mod panel;
pub mod panel_lvgl;
