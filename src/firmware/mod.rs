pub(crate) mod app_state;
#[cfg(feature = "ble-foundation")]
pub(crate) mod ble;
pub(crate) mod config;
mod display;
pub(crate) mod event_engine;
pub(crate) mod flash;
pub(crate) mod imu;
pub(crate) mod input;
#[cfg(feature = "asset-upload-http")]
pub(crate) mod net;
#[cfg(feature = "asset-upload-http")]
pub(crate) mod net_host;
pub(crate) mod observability;
pub(crate) mod panel_bus;
mod power;
pub(crate) mod psram;
pub(crate) mod scheduling;
pub(crate) mod self_test;
mod serial;
mod serial_uart;
pub(crate) mod service_mode;
mod storage;
mod system;
mod touch;
pub(crate) mod types;
pub(crate) mod ui;
pub(crate) mod update;

pub use system::run;

pub(crate) fn reset_pending_update_or_halt() -> ! {
    let pending = update::status().is_ok_and(|status| {
        status.image_state == Some(esp_bootloader_esp_idf::ota::OtaImageState::PendingVerify)
    });
    if pending {
        console::println!(
            "runtime: startup allocation failed during pending verification; rebooting for rollback"
        );
        esp_hal::system::software_reset();
    }
    loop {
        core::hint::spin_loop();
    }
}
