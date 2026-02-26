use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::types::SdUploadResultCode;

static WIFI_CONNECT_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
static WIFI_CONNECT_SUCCESSES: AtomicU32 = AtomicU32::new(0);
static WIFI_CONNECT_FAILURES: AtomicU32 = AtomicU32::new(0);
static WIFI_REASON_NO_AP_FOUND: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_RUNS: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_EMPTY: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_TARGET_HITS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_ACCEPTS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_ACCEPT_ERRORS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_REQUEST_ERRORS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_HEADER_TIMEOUTS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_READ_BODY_ERRORS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_SD_BUSY_ERRORS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_REQUESTS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_BYTES: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_BODY_READ_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_BODY_READ_MS_MAX: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_REQUEST_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_REQUEST_MS_MAX: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_ERRORS: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_BUSY: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_TIMEOUTS: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_POWER_ON_FAILED: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_INIT_FAILED: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_SESSION_TIMEOUT_ABORTS: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_SESSION_MODE_OFF_ABORTS: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_BEGIN_COUNT: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_BEGIN_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_BEGIN_MS_MAX: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_CHUNK_COUNT: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_CHUNK_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_CHUNK_MS_MAX: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_COMMIT_COUNT: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_COMMIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_COMMIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_ABORT_COUNT: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_ABORT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_ABORT_MS_MAX: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_MKDIR_COUNT: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_MKDIR_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_MKDIR_MS_MAX: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_REMOVE_COUNT: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_REMOVE_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static SD_UPLOAD_RTT_REMOVE_MS_MAX: AtomicU32 = AtomicU32::new(0);
static WIFI_LINK_CONNECTED: AtomicBool = AtomicBool::new(false);
static UPLOAD_HTTP_LISTENING: AtomicBool = AtomicBool::new(false);
static UPLOAD_HTTP_IPV4: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
pub(crate) enum SdUploadRoundtripPhase {
    Begin,
    Chunk,
    Commit,
    Abort,
    Mkdir,
    Remove,
}

#[derive(Clone, Copy)]
pub(crate) struct Snapshot {
    pub(crate) wifi_connect_attempts: u32,
    pub(crate) wifi_connect_successes: u32,
    pub(crate) wifi_connect_failures: u32,
    pub(crate) wifi_reason_no_ap_found: u32,
    pub(crate) wifi_scan_runs: u32,
    pub(crate) wifi_scan_empty: u32,
    pub(crate) wifi_scan_target_hits: u32,
    pub(crate) upload_http_accepts: u32,
    pub(crate) upload_http_accept_errors: u32,
    pub(crate) upload_http_request_errors: u32,
    pub(crate) upload_http_header_timeouts: u32,
    pub(crate) upload_http_read_body_errors: u32,
    pub(crate) upload_http_sd_busy_errors: u32,
    pub(crate) upload_http_upload_requests: u32,
    pub(crate) upload_http_upload_bytes: u32,
    pub(crate) upload_http_upload_body_read_ms_total: u32,
    pub(crate) upload_http_upload_body_read_ms_max: u32,
    pub(crate) upload_http_upload_sd_wait_ms_total: u32,
    pub(crate) upload_http_upload_sd_wait_ms_max: u32,
    pub(crate) upload_http_upload_request_ms_total: u32,
    pub(crate) upload_http_upload_request_ms_max: u32,
    pub(crate) sd_upload_errors: u32,
    pub(crate) sd_upload_busy: u32,
    pub(crate) sd_upload_timeouts: u32,
    pub(crate) sd_upload_power_on_failed: u32,
    pub(crate) sd_upload_init_failed: u32,
    pub(crate) sd_upload_session_timeout_aborts: u32,
    pub(crate) sd_upload_session_mode_off_aborts: u32,
    pub(crate) sd_upload_rtt_begin_count: u32,
    pub(crate) sd_upload_rtt_begin_ms_total: u32,
    pub(crate) sd_upload_rtt_begin_ms_max: u32,
    pub(crate) sd_upload_rtt_chunk_count: u32,
    pub(crate) sd_upload_rtt_chunk_ms_total: u32,
    pub(crate) sd_upload_rtt_chunk_ms_max: u32,
    pub(crate) sd_upload_rtt_commit_count: u32,
    pub(crate) sd_upload_rtt_commit_ms_total: u32,
    pub(crate) sd_upload_rtt_commit_ms_max: u32,
    pub(crate) sd_upload_rtt_abort_count: u32,
    pub(crate) sd_upload_rtt_abort_ms_total: u32,
    pub(crate) sd_upload_rtt_abort_ms_max: u32,
    pub(crate) sd_upload_rtt_mkdir_count: u32,
    pub(crate) sd_upload_rtt_mkdir_ms_total: u32,
    pub(crate) sd_upload_rtt_mkdir_ms_max: u32,
    pub(crate) sd_upload_rtt_remove_count: u32,
    pub(crate) sd_upload_rtt_remove_ms_total: u32,
    pub(crate) sd_upload_rtt_remove_ms_max: u32,
    pub(crate) wifi_link_connected: bool,
    pub(crate) upload_http_listening: bool,
    pub(crate) upload_http_ipv4: Option<[u8; 4]>,
}

pub(crate) fn snapshot() -> Snapshot {
    let upload_http_ipv4_raw = UPLOAD_HTTP_IPV4.load(Ordering::Relaxed);
    let upload_http_ipv4 = if upload_http_ipv4_raw == 0 {
        None
    } else {
        Some(upload_http_ipv4_raw.to_be_bytes())
    };
    Snapshot {
        wifi_connect_attempts: WIFI_CONNECT_ATTEMPTS.load(Ordering::Relaxed),
        wifi_connect_successes: WIFI_CONNECT_SUCCESSES.load(Ordering::Relaxed),
        wifi_connect_failures: WIFI_CONNECT_FAILURES.load(Ordering::Relaxed),
        wifi_reason_no_ap_found: WIFI_REASON_NO_AP_FOUND.load(Ordering::Relaxed),
        wifi_scan_runs: WIFI_SCAN_RUNS.load(Ordering::Relaxed),
        wifi_scan_empty: WIFI_SCAN_EMPTY.load(Ordering::Relaxed),
        wifi_scan_target_hits: WIFI_SCAN_TARGET_HITS.load(Ordering::Relaxed),
        upload_http_accepts: UPLOAD_HTTP_ACCEPTS.load(Ordering::Relaxed),
        upload_http_accept_errors: UPLOAD_HTTP_ACCEPT_ERRORS.load(Ordering::Relaxed),
        upload_http_request_errors: UPLOAD_HTTP_REQUEST_ERRORS.load(Ordering::Relaxed),
        upload_http_header_timeouts: UPLOAD_HTTP_HEADER_TIMEOUTS.load(Ordering::Relaxed),
        upload_http_read_body_errors: UPLOAD_HTTP_READ_BODY_ERRORS.load(Ordering::Relaxed),
        upload_http_sd_busy_errors: UPLOAD_HTTP_SD_BUSY_ERRORS.load(Ordering::Relaxed),
        upload_http_upload_requests: UPLOAD_HTTP_UPLOAD_REQUESTS.load(Ordering::Relaxed),
        upload_http_upload_bytes: UPLOAD_HTTP_UPLOAD_BYTES.load(Ordering::Relaxed),
        upload_http_upload_body_read_ms_total: UPLOAD_HTTP_UPLOAD_BODY_READ_MS_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_body_read_ms_max: UPLOAD_HTTP_UPLOAD_BODY_READ_MS_MAX
            .load(Ordering::Relaxed),
        upload_http_upload_sd_wait_ms_total: UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_sd_wait_ms_max: UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_MAX
            .load(Ordering::Relaxed),
        upload_http_upload_request_ms_total: UPLOAD_HTTP_UPLOAD_REQUEST_MS_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_request_ms_max: UPLOAD_HTTP_UPLOAD_REQUEST_MS_MAX
            .load(Ordering::Relaxed),
        sd_upload_errors: SD_UPLOAD_ERRORS.load(Ordering::Relaxed),
        sd_upload_busy: SD_UPLOAD_BUSY.load(Ordering::Relaxed),
        sd_upload_timeouts: SD_UPLOAD_TIMEOUTS.load(Ordering::Relaxed),
        sd_upload_power_on_failed: SD_UPLOAD_POWER_ON_FAILED.load(Ordering::Relaxed),
        sd_upload_init_failed: SD_UPLOAD_INIT_FAILED.load(Ordering::Relaxed),
        sd_upload_session_timeout_aborts: SD_UPLOAD_SESSION_TIMEOUT_ABORTS.load(Ordering::Relaxed),
        sd_upload_session_mode_off_aborts: SD_UPLOAD_SESSION_MODE_OFF_ABORTS
            .load(Ordering::Relaxed),
        sd_upload_rtt_begin_count: SD_UPLOAD_RTT_BEGIN_COUNT.load(Ordering::Relaxed),
        sd_upload_rtt_begin_ms_total: SD_UPLOAD_RTT_BEGIN_MS_TOTAL.load(Ordering::Relaxed),
        sd_upload_rtt_begin_ms_max: SD_UPLOAD_RTT_BEGIN_MS_MAX.load(Ordering::Relaxed),
        sd_upload_rtt_chunk_count: SD_UPLOAD_RTT_CHUNK_COUNT.load(Ordering::Relaxed),
        sd_upload_rtt_chunk_ms_total: SD_UPLOAD_RTT_CHUNK_MS_TOTAL.load(Ordering::Relaxed),
        sd_upload_rtt_chunk_ms_max: SD_UPLOAD_RTT_CHUNK_MS_MAX.load(Ordering::Relaxed),
        sd_upload_rtt_commit_count: SD_UPLOAD_RTT_COMMIT_COUNT.load(Ordering::Relaxed),
        sd_upload_rtt_commit_ms_total: SD_UPLOAD_RTT_COMMIT_MS_TOTAL.load(Ordering::Relaxed),
        sd_upload_rtt_commit_ms_max: SD_UPLOAD_RTT_COMMIT_MS_MAX.load(Ordering::Relaxed),
        sd_upload_rtt_abort_count: SD_UPLOAD_RTT_ABORT_COUNT.load(Ordering::Relaxed),
        sd_upload_rtt_abort_ms_total: SD_UPLOAD_RTT_ABORT_MS_TOTAL.load(Ordering::Relaxed),
        sd_upload_rtt_abort_ms_max: SD_UPLOAD_RTT_ABORT_MS_MAX.load(Ordering::Relaxed),
        sd_upload_rtt_mkdir_count: SD_UPLOAD_RTT_MKDIR_COUNT.load(Ordering::Relaxed),
        sd_upload_rtt_mkdir_ms_total: SD_UPLOAD_RTT_MKDIR_MS_TOTAL.load(Ordering::Relaxed),
        sd_upload_rtt_mkdir_ms_max: SD_UPLOAD_RTT_MKDIR_MS_MAX.load(Ordering::Relaxed),
        sd_upload_rtt_remove_count: SD_UPLOAD_RTT_REMOVE_COUNT.load(Ordering::Relaxed),
        sd_upload_rtt_remove_ms_total: SD_UPLOAD_RTT_REMOVE_MS_TOTAL.load(Ordering::Relaxed),
        sd_upload_rtt_remove_ms_max: SD_UPLOAD_RTT_REMOVE_MS_MAX.load(Ordering::Relaxed),
        wifi_link_connected: WIFI_LINK_CONNECTED.load(Ordering::Relaxed),
        upload_http_listening: UPLOAD_HTTP_LISTENING.load(Ordering::Relaxed),
        upload_http_ipv4,
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
    WIFI_LINK_CONNECTED.store(true, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::info!("telemetry wifi_connect_success");
}

pub(crate) fn record_wifi_connect_failure(reason: u8) {
    WIFI_CONNECT_FAILURES.fetch_add(1, Ordering::Relaxed);
    WIFI_LINK_CONNECTED.store(false, Ordering::Relaxed);
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

pub(crate) fn record_upload_http_accept() {
    UPLOAD_HTTP_ACCEPTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::trace!("telemetry upload_http_accept");
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

pub(crate) fn record_upload_http_request_bucket(error: &'static str) {
    match error {
        "request header timeout" => {
            UPLOAD_HTTP_HEADER_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
        }
        "read body" => {
            UPLOAD_HTTP_READ_BODY_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
        "sd busy" => {
            UPLOAD_HTTP_SD_BUSY_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

pub(crate) fn record_upload_http_upload_phase(
    bytes: u32,
    body_read_ms: u32,
    sd_wait_ms: u32,
    request_ms: u32,
) {
    UPLOAD_HTTP_UPLOAD_REQUESTS.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_BYTES, bytes);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_BODY_READ_MS_TOTAL, body_read_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_BODY_READ_MS_MAX, body_read_ms);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_TOTAL, sd_wait_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_MAX, sd_wait_ms);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_REQUEST_MS_TOTAL, request_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_REQUEST_MS_MAX, request_ms);
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

pub(crate) fn record_sd_upload_roundtrip_timing(phase: SdUploadRoundtripPhase, elapsed_ms: u32) {
    let (count, total, max) = match phase {
        SdUploadRoundtripPhase::Begin => (
            &SD_UPLOAD_RTT_BEGIN_COUNT,
            &SD_UPLOAD_RTT_BEGIN_MS_TOTAL,
            &SD_UPLOAD_RTT_BEGIN_MS_MAX,
        ),
        SdUploadRoundtripPhase::Chunk => (
            &SD_UPLOAD_RTT_CHUNK_COUNT,
            &SD_UPLOAD_RTT_CHUNK_MS_TOTAL,
            &SD_UPLOAD_RTT_CHUNK_MS_MAX,
        ),
        SdUploadRoundtripPhase::Commit => (
            &SD_UPLOAD_RTT_COMMIT_COUNT,
            &SD_UPLOAD_RTT_COMMIT_MS_TOTAL,
            &SD_UPLOAD_RTT_COMMIT_MS_MAX,
        ),
        SdUploadRoundtripPhase::Abort => (
            &SD_UPLOAD_RTT_ABORT_COUNT,
            &SD_UPLOAD_RTT_ABORT_MS_TOTAL,
            &SD_UPLOAD_RTT_ABORT_MS_MAX,
        ),
        SdUploadRoundtripPhase::Mkdir => (
            &SD_UPLOAD_RTT_MKDIR_COUNT,
            &SD_UPLOAD_RTT_MKDIR_MS_TOTAL,
            &SD_UPLOAD_RTT_MKDIR_MS_MAX,
        ),
        SdUploadRoundtripPhase::Remove => (
            &SD_UPLOAD_RTT_REMOVE_COUNT,
            &SD_UPLOAD_RTT_REMOVE_MS_TOTAL,
            &SD_UPLOAD_RTT_REMOVE_MS_MAX,
        ),
    };
    count.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(total, elapsed_ms);
    update_max_u32(max, elapsed_ms);
}

pub(crate) fn record_sd_upload_session_timeout_abort() {
    SD_UPLOAD_SESSION_TIMEOUT_ABORTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry sd_upload_session_timeout_abort");
}

pub(crate) fn record_sd_upload_session_mode_off_abort() {
    SD_UPLOAD_SESSION_MODE_OFF_ABORTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry sd_upload_session_mode_off_abort");
}

pub(crate) fn set_wifi_link_connected(connected: bool) {
    WIFI_LINK_CONNECTED.store(connected, Ordering::Relaxed);
}

pub(crate) fn wifi_link_connected() -> bool {
    WIFI_LINK_CONNECTED.load(Ordering::Relaxed)
}

pub(crate) fn set_upload_http_listener(listening: bool, ip: Option<[u8; 4]>) {
    UPLOAD_HTTP_LISTENING.store(listening, Ordering::Relaxed);
    let raw_ip = ip.map(u32::from_be_bytes).unwrap_or(0);
    UPLOAD_HTTP_IPV4.store(raw_ip, Ordering::Relaxed);
}

fn saturating_add_u32(counter: &AtomicU32, value: u32) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(value))
    });
}

fn update_max_u32(max_counter: &AtomicU32, value: u32) {
    let mut current = max_counter.load(Ordering::Relaxed);
    while value > current {
        match max_counter.compare_exchange_weak(
            current,
            value,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(next) => current = next,
        }
    }
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
