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
pub(crate) mod observability;
pub(crate) mod panel_bus;
mod power;
pub(crate) mod psram;
pub(crate) mod scheduling;
pub(crate) mod self_test;
mod serial;
pub(crate) mod service_mode;
mod storage;
mod system;
mod touch;
pub(crate) mod types;
pub(crate) mod ui;
pub(crate) mod update;

pub use system::run;
