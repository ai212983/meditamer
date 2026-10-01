#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

use esp_backtrace as _;

// Moved from the root package's `src/lib.rs` in the product and target axis
// completion plan's Phase 3: the ESP-IDF bootloader reads this app
// descriptor from any bootable partition, so it belongs to the artifact,
// not the product -- and each `[[bin]]` needs its own now that they no
// longer share a `[lib]` crate to carry one copy for both (`bin/updater.rs`
// has its own call for the same reason).
esp_bootloader_esp_idf::esp_app_desc!();

mod system;

#[cfg(feature = "asset-upload-http")]
mod net_host;
#[cfg(feature = "panel-waveform-fixture")]
mod panel_waveform_fixture;
mod serial_uart;

#[esp_hal::main]
fn main() -> ! {
    system::run()
}
