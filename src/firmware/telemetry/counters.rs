use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::types::SdUploadResultCode;

static WIFI_CONNECT_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
static WIFI_CONNECT_SUCCESSES: AtomicU32 = AtomicU32::new(0);
static WIFI_CONNECT_FAILURES: AtomicU32 = AtomicU32::new(0);
static WIFI_REASON_NO_AP_FOUND: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_RUNS: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_EMPTY: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_TARGET_HITS: AtomicU32 = AtomicU32::new(0);
static WIFI_CONNECTED_WATCHDOG_DISCONNECTS: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_MODE_PAUSES: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_MODE_RESUMES: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_CREDENTIALS_RECEIVED: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_CREDENTIALS_CHANGED: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_CONFIG_APPLIED: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_START_OK: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_START_ERR: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_CONNECT_BEGIN: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_CONNECT_SUCCESS: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_CONNECT_FAILURE: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_DISCONNECT_EVENTS: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_CHANNEL_PROBES: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_AUTH_ROTATIONS: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_HINT_RETRIES: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_CONNECT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_CONNECT_MS_MAX: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_SCAN_ACTIVE_RUNS: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_SCAN_ACTIVE_EMPTY: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_SCAN_ACTIVE_HITS: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_SCAN_ACTIVE_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_SCAN_ACTIVE_MS_MAX: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_SCAN_PASSIVE_RUNS: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_SCAN_PASSIVE_EMPTY: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_SCAN_PASSIVE_HITS: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_SCAN_PASSIVE_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_SCAN_PASSIVE_MS_MAX: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_REASON_2: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_REASON_201: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_REASON_202: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_REASON_203: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_REASON_204: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_REASON_205: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_REASON_210: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_REASON_211: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_REASON_212: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_REASON_OTHER: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_LAST_REASON: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_LAST_AUTH_IDX: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_LAST_CHANNEL_HINT: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_LAST_PROBE_IDX: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_LAST_SCAN_CHANNEL: AtomicU32 = AtomicU32::new(0);
static WIFI_REASSOC_LAST_STAGE: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_ACCEPTS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_ACCEPT_ERRORS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_ACCEPT_LINK_RESETS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_REQUEST_ERRORS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_HEADER_TIMEOUTS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_READ_BODY_ERRORS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_SD_BUSY_ERRORS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_HEALTH_REQUESTS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_REQUESTS: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_BYTES: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_BODY_READ_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_BODY_READ_MS_MAX: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_REQUEST_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static UPLOAD_HTTP_UPLOAD_REQUEST_MS_MAX: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_DHCP_WAIT_COUNT: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_DHCP_WAIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_DHCP_WAIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_DHCP_READY_COUNT: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_GATE_WIFI_DOWN: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_GATE_LINK_DOWN: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_GATE_NO_IPV4: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_LISTENER_ON: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_LISTENER_OFF: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_ACCEPT_WAIT_COUNT: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_ACCEPT_WAIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
static NET_PIPELINE_ACCEPT_WAIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
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
static BOOT_RESET_REASON_CODE: AtomicU32 = AtomicU32::new(0);
static WIFI_LINK_CONNECTED: AtomicBool = AtomicBool::new(false);
static UPLOAD_HTTP_LISTENING: AtomicBool = AtomicBool::new(false);
static UPLOAD_HTTP_IPV4: AtomicU32 = AtomicU32::new(0);
pub(crate) const DIAG_DOMAIN_WIFI: u32 = 1 << 0;
pub(crate) const DIAG_DOMAIN_REASSOC: u32 = 1 << 1;
pub(crate) const DIAG_DOMAIN_NET: u32 = 1 << 2;
pub(crate) const DIAG_DOMAIN_HTTP: u32 = 1 << 3;
pub(crate) const DIAG_DOMAIN_SD: u32 = 1 << 4;
pub(crate) const DIAG_MASK_ALL: u32 =
    DIAG_DOMAIN_WIFI | DIAG_DOMAIN_REASSOC | DIAG_DOMAIN_NET | DIAG_DOMAIN_HTTP | DIAG_DOMAIN_SD;
pub(crate) const DIAG_MASK_DEFAULT: u32 = DIAG_MASK_ALL;
static DIAG_MASK: AtomicU32 = AtomicU32::new(DIAG_MASK_DEFAULT);

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
pub(crate) enum WifiScanPhase {
    Active,
    Passive,
}

#[derive(Clone, Copy)]
pub(crate) enum NetPipelineGate {
    WifiDown,
    LinkDown,
    NoIpv4,
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
    pub(crate) wifi_connected_watchdog_disconnects: u32,
    pub(crate) wifi_reassoc_mode_pauses: u32,
    pub(crate) wifi_reassoc_mode_resumes: u32,
    pub(crate) wifi_reassoc_credentials_received: u32,
    pub(crate) wifi_reassoc_credentials_changed: u32,
    pub(crate) wifi_reassoc_config_applied: u32,
    pub(crate) wifi_reassoc_start_ok: u32,
    pub(crate) wifi_reassoc_start_err: u32,
    pub(crate) wifi_reassoc_connect_begin: u32,
    pub(crate) wifi_reassoc_connect_success: u32,
    pub(crate) wifi_reassoc_connect_failure: u32,
    pub(crate) wifi_reassoc_disconnect_events: u32,
    pub(crate) wifi_reassoc_channel_probes: u32,
    pub(crate) wifi_reassoc_auth_rotations: u32,
    pub(crate) wifi_reassoc_hint_retries: u32,
    pub(crate) wifi_reassoc_connect_ms_total: u32,
    pub(crate) wifi_reassoc_connect_ms_max: u32,
    pub(crate) wifi_reassoc_scan_active_runs: u32,
    pub(crate) wifi_reassoc_scan_active_empty: u32,
    pub(crate) wifi_reassoc_scan_active_hits: u32,
    pub(crate) wifi_reassoc_scan_active_ms_total: u32,
    pub(crate) wifi_reassoc_scan_active_ms_max: u32,
    pub(crate) wifi_reassoc_scan_passive_runs: u32,
    pub(crate) wifi_reassoc_scan_passive_empty: u32,
    pub(crate) wifi_reassoc_scan_passive_hits: u32,
    pub(crate) wifi_reassoc_scan_passive_ms_total: u32,
    pub(crate) wifi_reassoc_scan_passive_ms_max: u32,
    pub(crate) wifi_reassoc_reason_2: u32,
    pub(crate) wifi_reassoc_reason_201: u32,
    pub(crate) wifi_reassoc_reason_202: u32,
    pub(crate) wifi_reassoc_reason_203: u32,
    pub(crate) wifi_reassoc_reason_204: u32,
    pub(crate) wifi_reassoc_reason_205: u32,
    pub(crate) wifi_reassoc_reason_210: u32,
    pub(crate) wifi_reassoc_reason_211: u32,
    pub(crate) wifi_reassoc_reason_212: u32,
    pub(crate) wifi_reassoc_reason_other: u32,
    pub(crate) wifi_reassoc_last_reason: u8,
    pub(crate) wifi_reassoc_last_auth_idx: u8,
    pub(crate) wifi_reassoc_last_channel_hint: u8,
    pub(crate) wifi_reassoc_last_probe_idx: u8,
    pub(crate) wifi_reassoc_last_scan_channel: u8,
    pub(crate) wifi_reassoc_last_stage: u8,
    pub(crate) upload_http_accepts: u32,
    pub(crate) upload_http_accept_errors: u32,
    pub(crate) upload_http_accept_link_resets: u32,
    pub(crate) upload_http_request_errors: u32,
    pub(crate) upload_http_header_timeouts: u32,
    pub(crate) upload_http_read_body_errors: u32,
    pub(crate) upload_http_sd_busy_errors: u32,
    pub(crate) upload_http_health_requests: u32,
    pub(crate) upload_http_upload_requests: u32,
    pub(crate) upload_http_upload_bytes: u32,
    pub(crate) upload_http_upload_body_read_ms_total: u32,
    pub(crate) upload_http_upload_body_read_ms_max: u32,
    pub(crate) upload_http_upload_sd_wait_ms_total: u32,
    pub(crate) upload_http_upload_sd_wait_ms_max: u32,
    pub(crate) upload_http_upload_request_ms_total: u32,
    pub(crate) upload_http_upload_request_ms_max: u32,
    pub(crate) net_pipeline_dhcp_wait_count: u32,
    pub(crate) net_pipeline_dhcp_wait_ms_total: u32,
    pub(crate) net_pipeline_dhcp_wait_ms_max: u32,
    pub(crate) net_pipeline_dhcp_ready_count: u32,
    pub(crate) net_pipeline_gate_wifi_down: u32,
    pub(crate) net_pipeline_gate_link_down: u32,
    pub(crate) net_pipeline_gate_no_ipv4: u32,
    pub(crate) net_pipeline_listener_on: u32,
    pub(crate) net_pipeline_listener_off: u32,
    pub(crate) net_pipeline_accept_wait_count: u32,
    pub(crate) net_pipeline_accept_wait_ms_total: u32,
    pub(crate) net_pipeline_accept_wait_ms_max: u32,
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
    pub(crate) boot_reset_reason_code: u8,
    pub(crate) wifi_link_connected: bool,
    pub(crate) upload_http_listening: bool,
    pub(crate) upload_http_ipv4: Option<[u8; 4]>,
}

