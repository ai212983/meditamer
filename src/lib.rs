#![no_std]

esp_bootloader_esp_idf::esp_app_desc!();

// The UART console moved to the `console` platform crate (ADR-0015 step 1).
// This crate used to alias itself as `esp_println` so that `esp_println::println!`
// resolved here rather than to the upstream crate; only one crate per build can
// do that, which blocked every shared-crate extraction the ADR depends on.
// Call sites now say `console::println!`. These two re-exports keep the
// non-macro helpers reachable at the `crate::` paths their callers already use.
pub(crate) use console::{dropped_write_count, write_response as write_uart_response};
pub mod firmware;
pub mod platform;
#[cfg(feature = "factory-updater")]
pub mod updater;
