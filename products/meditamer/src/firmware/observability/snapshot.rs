use core::sync::atomic::Ordering;

use super::counters::*;
use super::types::Snapshot;

/// Compose the product's own snapshot with `platform/connectivity/netstack`'s
/// network-owned one (product/target axis completion plan, Phase 4). The
/// emitted field names and values are unchanged from before the split --
/// only their source moved. Every field exists unconditionally on
/// [`Snapshot`] (it always reported zero/false/none network state when
/// Wi-Fi was compiled out, before this split too), so the two builds share
/// the local-only fields and differ only in where the network-owned ones
/// come from; netstack retains its own network-owned `wifi_ipv4` in its
/// snapshot.
#[cfg(feature = "asset-upload-http")]
pub(crate) fn snapshot() -> Snapshot {
    let net = netstack::telemetry::snapshot();
    Snapshot {
        wifi_connect_attempts: net.wifi_connect_attempts,
        wifi_connect_successes: net.wifi_connect_successes,
        wifi_connect_failures: net.wifi_connect_failures,
        wifi_reason_no_ap_found: net.wifi_reason_no_ap_found,
        wifi_scan_runs: net.wifi_scan_runs,
        wifi_scan_empty: net.wifi_scan_empty,
        wifi_scan_target_hits: net.wifi_scan_target_hits,
        wifi_connected_watchdog_disconnects: net.wifi_connected_watchdog_disconnects,
        wifi_reassoc_mode_pauses: net.wifi_reassoc_mode_pauses,
        wifi_reassoc_mode_resumes: net.wifi_reassoc_mode_resumes,
        wifi_reassoc_credentials_received: net.wifi_reassoc_credentials_received,
        wifi_reassoc_credentials_changed: net.wifi_reassoc_credentials_changed,
        wifi_reassoc_config_applied: net.wifi_reassoc_config_applied,
        wifi_reassoc_start_ok: net.wifi_reassoc_start_ok,
        wifi_reassoc_start_err: net.wifi_reassoc_start_err,
        wifi_reassoc_connect_begin: net.wifi_reassoc_connect_begin,
        wifi_reassoc_connect_success: net.wifi_reassoc_connect_success,
        wifi_reassoc_connect_failure: net.wifi_reassoc_connect_failure,
        wifi_reassoc_disconnect_events: net.wifi_reassoc_disconnect_events,
        wifi_reassoc_channel_probes: net.wifi_reassoc_channel_probes,
        wifi_reassoc_auth_rotations: net.wifi_reassoc_auth_rotations,
        wifi_reassoc_hint_retries: net.wifi_reassoc_hint_retries,
        wifi_reassoc_connect_ms_total: net.wifi_reassoc_connect_ms_total,
        wifi_reassoc_connect_ms_max: net.wifi_reassoc_connect_ms_max,
        wifi_reassoc_scan_active_runs: net.wifi_reassoc_scan_active_runs,
        wifi_reassoc_scan_active_empty: net.wifi_reassoc_scan_active_empty,
        wifi_reassoc_scan_active_hits: net.wifi_reassoc_scan_active_hits,
        wifi_reassoc_scan_active_ms_total: net.wifi_reassoc_scan_active_ms_total,
        wifi_reassoc_scan_active_ms_max: net.wifi_reassoc_scan_active_ms_max,
        wifi_reassoc_scan_passive_runs: net.wifi_reassoc_scan_passive_runs,
        wifi_reassoc_scan_passive_empty: net.wifi_reassoc_scan_passive_empty,
        wifi_reassoc_scan_passive_hits: net.wifi_reassoc_scan_passive_hits,
        wifi_reassoc_scan_passive_ms_total: net.wifi_reassoc_scan_passive_ms_total,
        wifi_reassoc_scan_passive_ms_max: net.wifi_reassoc_scan_passive_ms_max,
        wifi_reassoc_reason_2: net.wifi_reassoc_reason_2,
        wifi_reassoc_reason_201: net.wifi_reassoc_reason_201,
        wifi_reassoc_reason_202: net.wifi_reassoc_reason_202,
        wifi_reassoc_reason_203: net.wifi_reassoc_reason_203,
        wifi_reassoc_reason_204: net.wifi_reassoc_reason_204,
        wifi_reassoc_reason_205: net.wifi_reassoc_reason_205,
        wifi_reassoc_reason_210: net.wifi_reassoc_reason_210,
        wifi_reassoc_reason_211: net.wifi_reassoc_reason_211,
        wifi_reassoc_reason_212: net.wifi_reassoc_reason_212,
        wifi_reassoc_reason_other: net.wifi_reassoc_reason_other,
        wifi_reassoc_last_reason: net.wifi_reassoc_last_reason,
        wifi_reassoc_last_auth_idx: net.wifi_reassoc_last_auth_idx,
        wifi_reassoc_last_channel_hint: net.wifi_reassoc_last_channel_hint,
        wifi_reassoc_last_probe_idx: net.wifi_reassoc_last_probe_idx,
        wifi_reassoc_last_scan_channel: net.wifi_reassoc_last_scan_channel,
        wifi_reassoc_last_stage: net.wifi_reassoc_last_stage,
        net_pipeline_dhcp_wait_count: net.net_pipeline_dhcp_wait_count,
        net_pipeline_dhcp_wait_ms_total: net.net_pipeline_dhcp_wait_ms_total,
        net_pipeline_dhcp_wait_ms_max: net.net_pipeline_dhcp_wait_ms_max,
        net_pipeline_dhcp_ready_count: net.net_pipeline_dhcp_ready_count,
        net_pipeline_gate_wifi_down: net.net_pipeline_gate_wifi_down,
        net_pipeline_gate_link_down: net.net_pipeline_gate_link_down,
        net_pipeline_gate_no_ipv4: net.net_pipeline_gate_no_ipv4,
        net_pipeline_listener_on: net.net_pipeline_listener_on,
        net_pipeline_listener_off: net.net_pipeline_listener_off,
        net_pipeline_accept_wait_count: net.net_pipeline_accept_wait_count,
        net_pipeline_accept_wait_ms_total: net.net_pipeline_accept_wait_ms_total,
        net_pipeline_accept_wait_ms_max: net.net_pipeline_accept_wait_ms_max,
        net_pipeline_accept_arm_gap_count: net.net_pipeline_accept_arm_gap_count,
        net_pipeline_accept_arm_gap_us_total: net.net_pipeline_accept_arm_gap_us_total,
        net_pipeline_accept_arm_gap_us_max: net.net_pipeline_accept_arm_gap_us_max,
        net_pipeline_accept_arm_gap_after_mkdir_count: net
            .net_pipeline_accept_arm_gap_after_mkdir_count,
        net_pipeline_accept_arm_gap_after_mkdir_us_total: net
            .net_pipeline_accept_arm_gap_after_mkdir_us_total,
        net_pipeline_accept_arm_gap_after_mkdir_us_max: net
            .net_pipeline_accept_arm_gap_after_mkdir_us_max,
        wifi_link_rssi_last_dbm: net.wifi_link_rssi_last_dbm,
        wifi_link_rssi_min_dbm: net.wifi_link_rssi_min_dbm,
        wifi_link_rssi_max_dbm: net.wifi_link_rssi_max_dbm,
        wifi_link_rssi_samples: net.wifi_link_rssi_samples,
        wifi_link_rssi_low_samples: net.wifi_link_rssi_low_samples,
        wifi_link_connected: net.wifi_link_connected,
        upload_http_listening: net.upload_http_listening,
        upload_http_ipv4: net.upload_http_ipv4,
        ..local_snapshot()
    }
}

#[cfg(not(feature = "asset-upload-http"))]
pub(crate) fn snapshot() -> Snapshot {
    local_snapshot()
}

/// The fields this module owns directly, regardless of `asset-upload-http`:
/// the product's own upload-service and SD-roundtrip timing. Network-owned
/// fields are zeroed here; the `asset-upload-http` build above overwrites
/// them with `platform/connectivity/netstack`'s snapshot via struct-update syntax.
fn local_snapshot() -> Snapshot {
    Snapshot {
        wifi_connect_attempts: 0,
        wifi_connect_successes: 0,
        wifi_connect_failures: 0,
        wifi_reason_no_ap_found: 0,
        wifi_scan_runs: 0,
        wifi_scan_empty: 0,
        wifi_scan_target_hits: 0,
        wifi_connected_watchdog_disconnects: 0,
        wifi_reassoc_mode_pauses: 0,
        wifi_reassoc_mode_resumes: 0,
        wifi_reassoc_credentials_received: 0,
        wifi_reassoc_credentials_changed: 0,
        wifi_reassoc_config_applied: 0,
        wifi_reassoc_start_ok: 0,
        wifi_reassoc_start_err: 0,
        wifi_reassoc_connect_begin: 0,
        wifi_reassoc_connect_success: 0,
        wifi_reassoc_connect_failure: 0,
        wifi_reassoc_disconnect_events: 0,
        wifi_reassoc_channel_probes: 0,
        wifi_reassoc_auth_rotations: 0,
        wifi_reassoc_hint_retries: 0,
        wifi_reassoc_connect_ms_total: 0,
        wifi_reassoc_connect_ms_max: 0,
        wifi_reassoc_scan_active_runs: 0,
        wifi_reassoc_scan_active_empty: 0,
        wifi_reassoc_scan_active_hits: 0,
        wifi_reassoc_scan_active_ms_total: 0,
        wifi_reassoc_scan_active_ms_max: 0,
        wifi_reassoc_scan_passive_runs: 0,
        wifi_reassoc_scan_passive_empty: 0,
        wifi_reassoc_scan_passive_hits: 0,
        wifi_reassoc_scan_passive_ms_total: 0,
        wifi_reassoc_scan_passive_ms_max: 0,
        wifi_reassoc_reason_2: 0,
        wifi_reassoc_reason_201: 0,
        wifi_reassoc_reason_202: 0,
        wifi_reassoc_reason_203: 0,
        wifi_reassoc_reason_204: 0,
        wifi_reassoc_reason_205: 0,
        wifi_reassoc_reason_210: 0,
        wifi_reassoc_reason_211: 0,
        wifi_reassoc_reason_212: 0,
        wifi_reassoc_reason_other: 0,
        wifi_reassoc_last_reason: 0,
        wifi_reassoc_last_auth_idx: 0,
        wifi_reassoc_last_channel_hint: 0,
        wifi_reassoc_last_probe_idx: 0,
        wifi_reassoc_last_scan_channel: 0,
        wifi_reassoc_last_stage: 0,
        upload_http_accepts: UPLOAD_HTTP_ACCEPTS.load(Ordering::Relaxed),
        upload_http_accept_errors: UPLOAD_HTTP_ACCEPT_ERRORS.load(Ordering::Relaxed),
        upload_http_accept_link_resets: UPLOAD_HTTP_ACCEPT_LINK_RESETS.load(Ordering::Relaxed),
        upload_http_request_errors: UPLOAD_HTTP_REQUEST_ERRORS.load(Ordering::Relaxed),
        upload_http_header_timeouts: UPLOAD_HTTP_HEADER_TIMEOUTS.load(Ordering::Relaxed),
        upload_http_read_body_errors: UPLOAD_HTTP_READ_BODY_ERRORS.load(Ordering::Relaxed),
        upload_http_read_body_resets: UPLOAD_HTTP_READ_BODY_RESETS.load(Ordering::Relaxed),
        upload_http_sd_busy_errors: UPLOAD_HTTP_SD_BUSY_ERRORS.load(Ordering::Relaxed),
        upload_http_health_requests: UPLOAD_HTTP_HEALTH_REQUESTS.load(Ordering::Relaxed),
        upload_http_upload_requests: UPLOAD_HTTP_UPLOAD_REQUESTS.load(Ordering::Relaxed),
        upload_http_upload_bytes: UPLOAD_HTTP_UPLOAD_BYTES.load(Ordering::Relaxed),
        upload_http_upload_body_read_ms_total: UPLOAD_HTTP_UPLOAD_BODY_READ_MS_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_body_read_ms_max: UPLOAD_HTTP_UPLOAD_BODY_READ_MS_MAX
            .load(Ordering::Relaxed),
        upload_http_upload_payload_copy_ms_total: UPLOAD_HTTP_UPLOAD_PAYLOAD_COPY_MS_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_payload_copy_ms_max: UPLOAD_HTTP_UPLOAD_PAYLOAD_COPY_MS_MAX
            .load(Ordering::Relaxed),
        upload_http_upload_sd_queue_ms_total: UPLOAD_HTTP_UPLOAD_SD_QUEUE_MS_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_sd_queue_ms_max: UPLOAD_HTTP_UPLOAD_SD_QUEUE_MS_MAX
            .load(Ordering::Relaxed),
        upload_http_upload_sd_task_wait_ms_total: UPLOAD_HTTP_UPLOAD_SD_TASK_WAIT_MS_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_sd_task_wait_ms_max: UPLOAD_HTTP_UPLOAD_SD_TASK_WAIT_MS_MAX
            .load(Ordering::Relaxed),
        upload_http_upload_commit_ms_total: UPLOAD_HTTP_UPLOAD_COMMIT_MS_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_commit_ms_max: UPLOAD_HTTP_UPLOAD_COMMIT_MS_MAX.load(Ordering::Relaxed),
        upload_http_upload_chunk_p50_ms_max: UPLOAD_HTTP_UPLOAD_CHUNK_P50_MS_MAX
            .load(Ordering::Relaxed),
        upload_http_upload_chunk_p95_ms_max: UPLOAD_HTTP_UPLOAD_CHUNK_P95_MS_MAX
            .load(Ordering::Relaxed),
        upload_http_upload_chunk_max_ms_max: UPLOAD_HTTP_UPLOAD_CHUNK_MAX_MS_MAX
            .load(Ordering::Relaxed),
        upload_http_upload_chunk_samples_total: UPLOAD_HTTP_UPLOAD_CHUNK_SAMPLES_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_chunk_samples_dropped: UPLOAD_HTTP_UPLOAD_CHUNK_SAMPLES_DROPPED
            .load(Ordering::Relaxed),
        upload_http_upload_sd_wait_ms_total: UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_sd_wait_ms_max: UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_MAX
            .load(Ordering::Relaxed),
        upload_http_upload_request_ms_total: UPLOAD_HTTP_UPLOAD_REQUEST_MS_TOTAL
            .load(Ordering::Relaxed),
        upload_http_upload_request_ms_max: UPLOAD_HTTP_UPLOAD_REQUEST_MS_MAX
            .load(Ordering::Relaxed),
        net_pipeline_dhcp_wait_count: 0,
        net_pipeline_dhcp_wait_ms_total: 0,
        net_pipeline_dhcp_wait_ms_max: 0,
        net_pipeline_dhcp_ready_count: 0,
        net_pipeline_gate_wifi_down: 0,
        net_pipeline_gate_link_down: 0,
        net_pipeline_gate_no_ipv4: 0,
        net_pipeline_listener_on: 0,
        net_pipeline_listener_off: 0,
        net_pipeline_accept_wait_count: 0,
        net_pipeline_accept_wait_ms_total: 0,
        net_pipeline_accept_wait_ms_max: 0,
        net_pipeline_accept_arm_gap_count: 0,
        net_pipeline_accept_arm_gap_us_total: 0,
        net_pipeline_accept_arm_gap_us_max: 0,
        net_pipeline_accept_arm_gap_after_mkdir_count: 0,
        net_pipeline_accept_arm_gap_after_mkdir_us_total: 0,
        net_pipeline_accept_arm_gap_after_mkdir_us_max: 0,
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
        boot_reset_reason_code: BOOT_RESET_REASON_CODE.load(Ordering::Relaxed) as u8,
        wifi_link_rssi_last_dbm: 0,
        wifi_link_rssi_min_dbm: 0,
        wifi_link_rssi_max_dbm: 0,
        wifi_link_rssi_samples: 0,
        wifi_link_rssi_low_samples: 0,
        wifi_link_connected: false,
        upload_http_listening: false,
        upload_http_ipv4: None,
    }
}
