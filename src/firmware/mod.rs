pub(crate) mod app_state;
#[cfg(feature = "ble-foundation")]
pub(crate) mod ble;
pub(crate) mod config;
pub(crate) mod event_engine;
pub(crate) mod firmware_update;
pub(crate) mod flash;
pub(crate) mod imu;
pub(crate) mod input;
#[cfg(feature = "asset-upload-http")]
pub(crate) mod net;
pub(crate) mod panel_bus;
pub(crate) mod psram;
mod runtime;
mod storage;
pub(crate) mod telemetry;
mod touch;
pub(crate) mod types;
pub(crate) mod ui;

pub use runtime::run;
