pub mod artifacts;
pub mod asset_residency;
pub mod ble_phase1d;
pub mod ble_phase1s;
pub mod common;
pub mod flash_capture;
pub mod observation_fixture;
pub mod observation_sleep;
pub mod passive_monitor;
pub mod runtime_modes;
pub mod sdcard;
#[cfg(test)]
mod sdcard_tests;
pub mod serial;
pub mod signing_key;
pub mod single_production;
pub mod storage;
pub mod troubleshoot;
pub mod ui_lifecycle;
pub mod upload;
pub mod wifi;

pub mod thermal_aba;
pub mod trace_capture;

pub mod panel_soak;
