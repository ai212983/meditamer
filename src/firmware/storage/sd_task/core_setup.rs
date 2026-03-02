#[cfg(feature = "asset-upload-http")]
use embassy_futures::select::{select3, Either3};
#[cfg(feature = "asset-upload-http")]
use embassy_time::with_timeout;
use embassy_time::{Duration, Instant, Timer};
use sdcard::runtime as sd_ops;

#[cfg(feature = "asset-upload-http")]
use super::super::config::WIFI_CONFIG_REQUESTS;
#[cfg(feature = "asset-upload-http")]
use super::super::config::{SD_REQUESTS, SD_UPLOAD_REQUESTS};
#[cfg(feature = "asset-upload-http")]
use super::super::runtime::service_mode;
#[cfg(feature = "asset-upload-http")]
use super::super::telemetry;
use super::super::types::{SdCommand, SdPowerRequest, SdProbeDriver, SdRequest};
#[cfg(feature = "asset-upload-http")]
use super::super::types::{
    SdUploadCommand, SdUploadRequest, SdUploadResult, SdUploadResultCode, WifiConfigResponse,
    WifiConfigResultCode,
};

mod asset_read;
mod dispatch;
mod logging;
mod power;
#[cfg(not(feature = "asset-upload-http"))]
mod receive;
#[cfg(all(test, not(target_os = "none")))]
mod tests;
mod upload;
#[cfg(feature = "asset-upload-http")]
mod wifi_config;

use dispatch::process_request;
#[cfg(feature = "asset-upload-http")]
use logging::publish_upload_result;
#[cfg(feature = "asset-upload-http")]
use logging::publish_wifi_config_response;
use logging::{publish_result, sd_power_action_label};
pub(super) use power::{duration_ms_since, failure_backoff_ms, request_sd_power};
#[cfg(not(feature = "asset-upload-http"))]
use receive::receive_core_request;
#[cfg(feature = "asset-upload-http")]
use upload::process_upload_request;
use upload::SdUploadSession;
#[cfg(feature = "asset-upload-http")]
use wifi_config::process_wifi_config_request;

const SD_IDLE_POWER_OFF_MS: u64 = 1_500;
const SD_BOOT_POWER_OFF_GRACE_MS: u64 = 6_000;
const SD_RETRY_MAX_ATTEMPTS: u8 = 3;
const SD_RETRY_DELAY_MS: u64 = 120;
const SD_BACKOFF_BASE_MS: u64 = 300;
const SD_BACKOFF_MAX_MS: u64 = 2_400;
const SD_POWER_ON_RESPONSE_TIMEOUT_MS: u64 = 1_500;
const SD_POWER_OFF_RESPONSE_TIMEOUT_MS: u64 = 4_000;
const SD_POWER_REQUEST_ENQUEUE_TIMEOUT_MS: u64 = 1_500;
const SD_POWER_REQUEST_MAX_ATTEMPTS: u8 = 4;
const SD_POWER_REQUEST_RETRY_DELAY_MS: u64 = 120;
const SD_UPLOAD_TMP_BASENAME: &[u8] = b"HCTLUPLD.TMP";
const SD_UPLOAD_PATH_BUF_MAX: usize = 72;
const SD_UPLOAD_ROOT: &str = "/assets";
#[cfg(feature = "asset-upload-http")]
const SD_UPLOAD_SESSION_IDLE_ABORT_MS: u32 = 120_000;
#[cfg(feature = "asset-upload-http")]
const WIFI_CONFIG_DIR: &str = "/config";
#[cfg(feature = "asset-upload-http")]
const WIFI_CONFIG_PATH: &str = "/config/wifi.cfg";

