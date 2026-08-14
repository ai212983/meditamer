#![no_std]

extern crate self as esp_println;

esp_bootloader_esp_idf::esp_app_desc!();

#[path = "esp_println.rs"]
mod uart_println;
pub use crate::{esp_uart_print as print, esp_uart_println as println};
pub(crate) use uart_println::{dropped_write_count, write_response as write_uart_response};
pub mod firmware;
pub mod platform;
