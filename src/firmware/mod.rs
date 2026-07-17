pub(crate) mod app_state;
pub mod assets;
pub(crate) mod config;
pub(crate) mod event_engine;
#[cfg(all(feature = "graphics", not(feature = "wifi-debug-slim-app")))]
pub mod graphics;
pub(crate) mod imu;
pub(crate) mod input;
pub(crate) mod psram;
mod render;
mod runtime;
mod storage;
pub(crate) mod telemetry;
mod touch;
pub(crate) mod types;
pub(crate) mod ui;

pub use runtime::run;
