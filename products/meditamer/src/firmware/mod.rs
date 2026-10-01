pub mod app_state;
pub mod battery;
#[cfg(feature = "ble-foundation")]
pub mod ble;
// Not `pub`: only `panel_bus` and the five clients it orchestrates (`imu`,
// `environment`, `battery`, `touch::tasks::{acquisition,pipeline}`) need
// this, and each of those is itself a descendant of `firmware`, which a
// private `mod` already reaches.
mod acquisition_metrics;
mod bounded_control;
pub mod config;
#[cfg(feature = "cpu-load")]
pub mod cpu_observation;
pub mod display;
pub mod environment;
pub mod event_engine;
pub mod flash;
pub mod imu;
pub mod input;
#[cfg(feature = "ui-interaction-trace")]
pub(crate) mod interaction_trace;
pub mod observability;
pub mod panel_bus;
pub mod psram;
pub mod scheduling;
pub mod self_test;
pub mod serial;
pub mod service_mode;
pub mod storage;
pub mod touch;
#[cfg(feature = "firmware-trace")]
pub mod trace;
pub mod types;
pub mod ui;
pub mod update;

// `system`/`system::tasks` (chip startup and Embassy task wiring) moved to
// `targets/meditamer-inkplate` in Phase 3 (product and target axis
// completion plan) -- this crate no longer exposes a `run()` entry point of
// its own; the target's `main.rs` is one now.

pub fn reset_pending_update_or_halt() -> ! {
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

pub(crate) mod observation_fixture;
