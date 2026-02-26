use core::sync::atomic::{AtomicU32, Ordering};

use super::types::SdUploadResultCode;

static WIFI_CONNECT_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
static WIFI_CONNECT_SUCCESSES: AtomicU32 = AtomicU32::new(0);
static WIFI_CONNECT_FAILURES: AtomicU32 = AtomicU32::new(0);
static WIFI_REASON_NO_AP_FOUND: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_RUNS: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_EMPTY: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_TARGET_HITS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_ACCEPT_ERRORS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_REQUEST_ERRORS: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_ERRORS: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_BUSY: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_TIMEOUTS: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_POWER_ON_FAILED: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_INIT_FAILED: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
pub(crate) struct Snapshot {
    pub(crate) wifi_connect_attempts: u32,
    pub(crate) wifi_connect_successes: u32,
    pub(crate) wifi_connect_failures: u32,
    pub(crate) wifi_reason_no_ap_found: u32,
    pub(crate) wifi_scan_runs: u32,
    pub(crate) wifi_scan_empty: u32,
    pub(crate) wifi_scan_target_hits: u32,
    pub(crate) upload_http_accept_errors: u32,
    pub(crate) upload_http_request_errors: u32,
    pub(crate) sd_upload_errors: u32,
    pub(crate) sd_upload_busy: u32,
    pub(crate) sd_upload_timeouts: u32,
    pub(crate) sd_upload_power_on_failed: u32,
    pub(crate) sd_upload_init_failed: u32,
}

pub(crate) fn snapshot() -> Snapshot {
    Snapshot {
        wifi_connect_attempts: WIFI_CONNECT_ATTEMPTS.load(Ordering::Relaxed),
        wifi_connect_successes: WIFI_CONNECT_SUCCESSES.load(Ordering::Relaxed),
        wifi_connect_failures: WIFI_CONNECT_FAILURES.load(Ordering::Relaxed),
        wifi_reason_no_ap_found: WIFI_REASON_NO_AP_FOUND.load(Ordering::Relaxed),
        wifi_scan_runs: WIFI_SCAN_RUNS.load(Ordering::Relaxed),
        wifi_scan_empty: WIFI_SCAN_EMPTY.load(Ordering::Relaxed),
        wifi_scan_target_hits: WIFI_SCAN_TARGET_HITS.load(Ordering::Relaxed),
        upload_http_accept_errors: UPLOAD_HTTP_ACCEPT_ERRORS.load(Ordering::Relaxed),
        upload_http_request_errors: UPLOAD_HTTP_REQUEST_ERRORS.load(Ordering::Relaxed),
        sd_upload_errors: SD_UPLOAD_ERRORS.load(Ordering::Relaxed),
        sd_upload_busy: SD_UPLOAD_BUSY.load(Ordering::Relaxed),
        sd_upload_timeouts: SD_UPLOAD_TIMEOUTS.load(Ordering::Relaxed),
        sd_upload_power_on_failed: SD_UPLOAD_POWER_ON_FAILED.load(Ordering::Relaxed),
        sd_upload_init_failed: SD_UPLOAD_INIT_FAILED.load(Ordering::Relaxed),
    }
}

pub(crate) fn record_wifi_connect_attempt(_channel_hint: Option<u8>, _auth_idx: usize) {
    WIFI_CONNECT_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::trace!(
        "telemetry wifi_connect_attempt auth_idx={=u8} channel_hint={=u8}",
        _auth_idx as u8,
        _channel_hint.unwrap_or(0),
    );
}

pub(crate) fn record_wifi_connect_success() {
    WIFI_CONNECT_SUCCESSES.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::info!("telemetry wifi_connect_success");
}

pub(crate) fn record_wifi_connect_failure(reason: u8) {
    WIFI_CONNECT_FAILURES.fetch_add(1, Ordering::Relaxed);
    if reason == 201 {
        WIFI_REASON_NO_AP_FOUND.fetch_add(1, Ordering::Relaxed);
    }
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry wifi_connect_failure reason={=u8}", reason);
}

pub(crate) fn record_wifi_scan(result_count: usize, target_found: bool) {
    WIFI_SCAN_RUNS.fetch_add(1, Ordering::Relaxed);
    if result_count == 0 {
        WIFI_SCAN_EMPTY.fetch_add(1, Ordering::Relaxed);
    }
    if target_found {
        WIFI_SCAN_TARGET_HITS.fetch_add(1, Ordering::Relaxed);
    }
    #[cfg(feature = "telemetry-defmt")]
    defmt::debug!(
        "telemetry wifi_scan result_count={=u16} target_found={=bool}",
        result_count as u16,
        target_found,
    );
}

pub(crate) fn record_upload_http_accept_error() {
    UPLOAD_HTTP_ACCEPT_ERRORS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry upload_http_accept_error");
}

pub(crate) fn record_upload_http_request_error() {
    UPLOAD_HTTP_REQUEST_ERRORS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry upload_http_request_error");
}

pub(crate) fn record_sd_upload_roundtrip_timeout() {
    SD_UPLOAD_ERRORS.fetch_add(1, Ordering::Relaxed);
    SD_UPLOAD_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry sd_upload_roundtrip_timeout");
}

pub(crate) fn record_sd_upload_roundtrip_code(code: SdUploadResultCode) {
    SD_UPLOAD_ERRORS.fetch_add(1, Ordering::Relaxed);
    match code {
        SdUploadResultCode::Busy => {
            SD_UPLOAD_BUSY.fetch_add(1, Ordering::Relaxed);
        }
        SdUploadResultCode::PowerOnFailed => {
            SD_UPLOAD_POWER_ON_FAILED.fetch_add(1, Ordering::Relaxed);
        }
        SdUploadResultCode::InitFailed => {
            SD_UPLOAD_INIT_FAILED.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!(
        "telemetry sd_upload_roundtrip_code code={=u8}",
        sd_upload_result_code_to_u8(code),
    );
}

#[cfg(feature = "telemetry-defmt")]
fn sd_upload_result_code_to_u8(code: SdUploadResultCode) -> u8 {
    match code {
        SdUploadResultCode::Ok => 0,
        SdUploadResultCode::Busy => 1,
        SdUploadResultCode::SessionNotActive => 2,
        SdUploadResultCode::InvalidPath => 3,
        SdUploadResultCode::NotFound => 4,
        SdUploadResultCode::NotEmpty => 5,
        SdUploadResultCode::SizeMismatch => 6,
        SdUploadResultCode::PowerOnFailed => 7,
        SdUploadResultCode::InitFailed => 8,
        SdUploadResultCode::OperationFailed => 9,
    }
}
