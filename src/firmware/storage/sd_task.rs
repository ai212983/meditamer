//! SD task: owns the card, runs the request loop, and publishes results.
//!
//! [`runtime_loop`] is the state machine that drives one power/request cycle;
//! [`upload_ready`] gates the upload path. This file declares the task's
//! submodules and the constants they share.

use super::super::types::{SdCommand, SdPowerRequest, SdProbeDriver, SdRequest};

mod dispatch;
mod engine_driver;
mod logging;
mod manual_io;
mod power;
#[cfg(not(feature = "asset-upload-http"))]
mod receive;
mod runtime_loop;
mod runtime_startup;
mod serial_log;
#[cfg(all(test, not(target_os = "none")))]
mod tests;
mod upload;
mod upload_ready;
#[cfg(feature = "asset-upload-http")]
mod wifi_config;

use dispatch::process_request;
use logging::{publish_result, sd_power_action_label};
pub(super) use power::{duration_ms_since, failure_backoff_ms, request_sd_power};

pub const SD_IDLE_POWER_OFF_MS: u64 = 1_500;
pub(super) const SD_BOOT_POWER_OFF_GRACE_MS: u64 = 6_000;
pub(super) const SD_RETRY_MAX_ATTEMPTS: u8 = 3;
pub(super) const SD_RETRY_DELAY_MS: u64 = 120;
pub(super) const SD_BACKOFF_BASE_MS: u64 = 300;
pub(super) const SD_BACKOFF_MAX_MS: u64 = 2_400;
pub(super) const SD_POWER_ON_RESPONSE_TIMEOUT_MS: u64 = 1_500;
pub(super) const SD_POWER_OFF_RESPONSE_TIMEOUT_MS: u64 = 4_000;
pub(super) const SD_POWER_REQUEST_ENQUEUE_TIMEOUT_MS: u64 = 1_500;
pub(super) const SD_POWER_REQUEST_MAX_ATTEMPTS: u8 = 4;
pub(super) const SD_POWER_REQUEST_RETRY_DELAY_MS: u64 = 120;
pub const SD_UPLOAD_TMP_BASENAME: &[u8] = b"HCTLUPLD.TMP";
pub const SD_UPLOAD_PATH_BUF_MAX: usize = 72;
pub const SD_UPLOAD_ROOT: &str = "/assets";
#[cfg(feature = "asset-upload-http")]
pub(super) const SD_UPLOAD_SESSION_IDLE_ABORT_MS: u32 = 120_000;
#[cfg(feature = "asset-upload-http")]
pub(super) const WIFI_CONFIG_DIR: &str = "/config";
#[cfg(feature = "asset-upload-http")]
pub(super) const WIFI_CONFIG_PATH: &str = "/config/wifi.cfg";

pub(crate) use runtime_loop::sd_task;
