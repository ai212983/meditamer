#![no_std]
//! Host-testable PCF85063A wall-clock driver.
//!
//! Unlike `packages/sdcard`, nothing here needs the Xtensa toolchain or
//! `esp-hal`: the driver is generic over `embedded_hal_async::i2c::I2c`, so
//! the same code compiles and runs identically on the ESP32 target and on
//! the host under `cargo test --features host-tests`. No `target_os` gating
//! is required.

pub mod calendar;
pub mod driver;
pub mod offset;
pub mod registers;
