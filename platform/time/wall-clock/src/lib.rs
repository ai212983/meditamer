#![no_std]
//! Transport-neutral cross-board wall-clock synchronization.
//!
//! Owns synchronization policy, session state, typed messages, RTC
//! application, and delayed verification -- everything above the shared
//! [`rtc`] driver that Inkplate and Waveshare should not each reimplement.
//! A transport (serial, ...) only carries [`codec`]-encoded lines; it has no
//! direct RTC access. See `docs/references/wall-clock-sync.md`.
//!
//! Host-testable like `platform/time/rtc`: generic over
//! `embedded_hal_async::i2c::I2c` (via `rtc`) and
//! `embedded_hal_async::delay::DelayNs`, so the same code compiles and runs
//! identically on-device and under `cargo test`.

pub mod codec;
pub mod message;
pub mod policy;
pub mod rtc_backend;
pub mod session;
