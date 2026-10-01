use core::sync::atomic::Ordering;

use super::counters::*;

#[derive(Clone, Copy)]
pub struct NetSnapshot {
    pub wifi_connect_attempts: u32,
    pub wifi_connect_successes: u32,
    pub wifi_connect_failures: u32,
    pub wifi_reason_no_ap_found: u32,
    pub wifi_scan_runs: u32,
    pub wifi_scan_empty: u32,
    pub wifi_scan_target_hits: u32,
    pub wifi_connected_watchdog_disconnects: u32,
    pub wifi_reassoc_mode_pauses: u32,
    pub wifi_reassoc_mode_resumes: u32,
    pub wifi_reassoc_credentials_received: u32,
    pub wifi_reassoc_credentials_changed: u32,
    pub wifi_reassoc_config_applied: u32,
    pub wifi_reassoc_start_ok: u32,
    pub wifi_reassoc_start_err: u32,
    pub wifi_reassoc_connect_begin: u32,
    pub wifi_reassoc_connect_success: u32,
    pub wifi_reassoc_connect_failure: u32,
    pub wifi_reassoc_disconnect_events: u32,
    pub wifi_reassoc_channel_probes: u32,
    pub wifi_reassoc_auth_rotations: u32,
    pub wifi_reassoc_hint_retries: u32,
    pub wifi_reassoc_connect_ms_total: u32,
    pub wifi_reassoc_connect_ms_max: u32,
    pub wifi_reassoc_scan_active_runs: u32,
    pub wifi_reassoc_scan_active_empty: u32,
    pub wifi_reassoc_scan_active_hits: u32,
    pub wifi_reassoc_scan_active_ms_total: u32,
    pub wifi_reassoc_scan_active_ms_max: u32,
    pub wifi_reassoc_scan_passive_runs: u32,
    pub wifi_reassoc_scan_passive_empty: u32,
    pub wifi_reassoc_scan_passive_hits: u32,
    pub wifi_reassoc_scan_passive_ms_total: u32,
    pub wifi_reassoc_scan_passive_ms_max: u32,
    pub wifi_reassoc_reason_2: u32,
    pub wifi_reassoc_reason_201: u32,
    pub wifi_reassoc_reason_202: u32,
    pub wifi_reassoc_reason_203: u32,
    pub wifi_reassoc_reason_204: u32,
    pub wifi_reassoc_reason_205: u32,
    pub wifi_reassoc_reason_210: u32,
    pub wifi_reassoc_reason_211: u32,
    pub wifi_reassoc_reason_212: u32,
    pub wifi_reassoc_reason_other: u32,
    pub wifi_reassoc_last_reason: u8,
    pub wifi_reassoc_last_auth_idx: u8,
    pub wifi_reassoc_last_channel_hint: u8,
    pub wifi_reassoc_last_probe_idx: u8,
    pub wifi_reassoc_last_scan_channel: u8,
    pub wifi_reassoc_last_stage: u8,
    pub net_pipeline_dhcp_wait_count: u32,
    pub net_pipeline_dhcp_wait_ms_total: u32,
    pub net_pipeline_dhcp_wait_ms_max: u32,
    pub net_pipeline_dhcp_ready_count: u32,
    pub net_pipeline_gate_wifi_down: u32,
    pub net_pipeline_gate_link_down: u32,
    pub net_pipeline_gate_no_ipv4: u32,
    pub net_pipeline_listener_on: u32,
    pub net_pipeline_listener_off: u32,
    pub net_pipeline_accept_wait_count: u32,
    pub net_pipeline_accept_wait_ms_total: u32,
    pub net_pipeline_accept_wait_ms_max: u32,
    pub net_pipeline_accept_arm_gap_count: u32,
    pub net_pipeline_accept_arm_gap_us_total: u32,
    pub net_pipeline_accept_arm_gap_us_max: u32,
    pub net_pipeline_accept_arm_gap_after_mkdir_count: u32,
    pub net_pipeline_accept_arm_gap_after_mkdir_us_total: u32,
    pub net_pipeline_accept_arm_gap_after_mkdir_us_max: u32,
    pub wifi_link_rssi_last_dbm: i32,
    pub wifi_link_rssi_min_dbm: i32,
    pub wifi_link_rssi_max_dbm: i32,
    pub wifi_link_rssi_samples: u32,
    pub wifi_link_rssi_low_samples: u32,
    pub wifi_link_connected: bool,
    pub wifi_ipv4: Option<[u8; 4]>,
    pub upload_http_listening: bool,
    pub upload_http_ipv4: Option<[u8; 4]>,
}

pub fn snapshot() -> NetSnapshot {
    fn decode_rssi_dbm(encoded: u32) -> i32 {
        if encoded == u32::MAX {
            0
        } else {
            encoded as i32 - 128
        }
    }

    fn decode_ipv4(raw: u32) -> Option<[u8; 4]> {
        if raw == 0 {
            None
        } else {
            Some(raw.to_be_bytes())
        }
    }

    let wifi_ipv4 = decode_ipv4(WIFI_IPV4.load(Ordering::Relaxed));
    let upload_http_ipv4 = decode_ipv4(UPLOAD_HTTP_IPV4.load(Ordering::Relaxed));
    NetSnapshot {
        wifi_connect_attempts: WIFI_CONNECT_ATTEMPTS.load(Ordering::Relaxed),
        wifi_connect_successes: WIFI_CONNECT_SUCCESSES.load(Ordering::Relaxed),
        wifi_connect_failures: WIFI_CONNECT_FAILURES.load(Ordering::Relaxed),
        wifi_reason_no_ap_found: WIFI_REASON_NO_AP_FOUND.load(Ordering::Relaxed),
        wifi_scan_runs: WIFI_SCAN_RUNS.load(Ordering::Relaxed),
        wifi_scan_empty: WIFI_SCAN_EMPTY.load(Ordering::Relaxed),
        wifi_scan_target_hits: WIFI_SCAN_TARGET_HITS.load(Ordering::Relaxed),
        wifi_connected_watchdog_disconnects: WIFI_CONNECTED_WATCHDOG_DISCONNECTS
            .load(Ordering::Relaxed),
        wifi_reassoc_mode_pauses: WIFI_REASSOC_MODE_PAUSES.load(Ordering::Relaxed),
        wifi_reassoc_mode_resumes: WIFI_REASSOC_MODE_RESUMES.load(Ordering::Relaxed),
        wifi_reassoc_credentials_received: WIFI_REASSOC_CREDENTIALS_RECEIVED
            .load(Ordering::Relaxed),
        wifi_reassoc_credentials_changed: WIFI_REASSOC_CREDENTIALS_CHANGED.load(Ordering::Relaxed),
        wifi_reassoc_config_applied: WIFI_REASSOC_CONFIG_APPLIED.load(Ordering::Relaxed),
        wifi_reassoc_start_ok: WIFI_REASSOC_START_OK.load(Ordering::Relaxed),
        wifi_reassoc_start_err: WIFI_REASSOC_START_ERR.load(Ordering::Relaxed),
        wifi_reassoc_connect_begin: WIFI_REASSOC_CONNECT_BEGIN.load(Ordering::Relaxed),
        wifi_reassoc_connect_success: WIFI_REASSOC_CONNECT_SUCCESS.load(Ordering::Relaxed),
        wifi_reassoc_connect_failure: WIFI_REASSOC_CONNECT_FAILURE.load(Ordering::Relaxed),
        wifi_reassoc_disconnect_events: WIFI_REASSOC_DISCONNECT_EVENTS.load(Ordering::Relaxed),
        wifi_reassoc_channel_probes: WIFI_REASSOC_CHANNEL_PROBES.load(Ordering::Relaxed),
        wifi_reassoc_auth_rotations: WIFI_REASSOC_AUTH_ROTATIONS.load(Ordering::Relaxed),
        wifi_reassoc_hint_retries: WIFI_REASSOC_HINT_RETRIES.load(Ordering::Relaxed),
        wifi_reassoc_connect_ms_total: WIFI_REASSOC_CONNECT_MS_TOTAL.load(Ordering::Relaxed),
        wifi_reassoc_connect_ms_max: WIFI_REASSOC_CONNECT_MS_MAX.load(Ordering::Relaxed),
        wifi_reassoc_scan_active_runs: WIFI_REASSOC_SCAN_ACTIVE_RUNS.load(Ordering::Relaxed),
        wifi_reassoc_scan_active_empty: WIFI_REASSOC_SCAN_ACTIVE_EMPTY.load(Ordering::Relaxed),
        wifi_reassoc_scan_active_hits: WIFI_REASSOC_SCAN_ACTIVE_HITS.load(Ordering::Relaxed),
        wifi_reassoc_scan_active_ms_total: WIFI_REASSOC_SCAN_ACTIVE_MS_TOTAL
            .load(Ordering::Relaxed),
        wifi_reassoc_scan_active_ms_max: WIFI_REASSOC_SCAN_ACTIVE_MS_MAX.load(Ordering::Relaxed),
        wifi_reassoc_scan_passive_runs: WIFI_REASSOC_SCAN_PASSIVE_RUNS.load(Ordering::Relaxed),
        wifi_reassoc_scan_passive_empty: WIFI_REASSOC_SCAN_PASSIVE_EMPTY.load(Ordering::Relaxed),
        wifi_reassoc_scan_passive_hits: WIFI_REASSOC_SCAN_PASSIVE_HITS.load(Ordering::Relaxed),
        wifi_reassoc_scan_passive_ms_total: WIFI_REASSOC_SCAN_PASSIVE_MS_TOTAL
            .load(Ordering::Relaxed),
        wifi_reassoc_scan_passive_ms_max: WIFI_REASSOC_SCAN_PASSIVE_MS_MAX.load(Ordering::Relaxed),
        wifi_reassoc_reason_2: WIFI_REASSOC_REASON_2.load(Ordering::Relaxed),
        wifi_reassoc_reason_201: WIFI_REASSOC_REASON_201.load(Ordering::Relaxed),
        wifi_reassoc_reason_202: WIFI_REASSOC_REASON_202.load(Ordering::Relaxed),
        wifi_reassoc_reason_203: WIFI_REASSOC_REASON_203.load(Ordering::Relaxed),
        wifi_reassoc_reason_204: WIFI_REASSOC_REASON_204.load(Ordering::Relaxed),
        wifi_reassoc_reason_205: WIFI_REASSOC_REASON_205.load(Ordering::Relaxed),
        wifi_reassoc_reason_210: WIFI_REASSOC_REASON_210.load(Ordering::Relaxed),
        wifi_reassoc_reason_211: WIFI_REASSOC_REASON_211.load(Ordering::Relaxed),
        wifi_reassoc_reason_212: WIFI_REASSOC_REASON_212.load(Ordering::Relaxed),
        wifi_reassoc_reason_other: WIFI_REASSOC_REASON_OTHER.load(Ordering::Relaxed),
        wifi_reassoc_last_reason: WIFI_REASSOC_LAST_REASON.load(Ordering::Relaxed) as u8,
        wifi_reassoc_last_auth_idx: WIFI_REASSOC_LAST_AUTH_IDX.load(Ordering::Relaxed) as u8,
        wifi_reassoc_last_channel_hint: WIFI_REASSOC_LAST_CHANNEL_HINT.load(Ordering::Relaxed)
            as u8,
        wifi_reassoc_last_probe_idx: WIFI_REASSOC_LAST_PROBE_IDX.load(Ordering::Relaxed) as u8,
        wifi_reassoc_last_scan_channel: WIFI_REASSOC_LAST_SCAN_CHANNEL.load(Ordering::Relaxed)
            as u8,
        wifi_reassoc_last_stage: WIFI_REASSOC_LAST_STAGE.load(Ordering::Relaxed) as u8,
        net_pipeline_dhcp_wait_count: NET_PIPELINE_DHCP_WAIT_COUNT.load(Ordering::Relaxed),
        net_pipeline_dhcp_wait_ms_total: NET_PIPELINE_DHCP_WAIT_MS_TOTAL.load(Ordering::Relaxed),
        net_pipeline_dhcp_wait_ms_max: NET_PIPELINE_DHCP_WAIT_MS_MAX.load(Ordering::Relaxed),
        net_pipeline_dhcp_ready_count: NET_PIPELINE_DHCP_READY_COUNT.load(Ordering::Relaxed),
        net_pipeline_gate_wifi_down: NET_PIPELINE_GATE_WIFI_DOWN.load(Ordering::Relaxed),
        net_pipeline_gate_link_down: NET_PIPELINE_GATE_LINK_DOWN.load(Ordering::Relaxed),
        net_pipeline_gate_no_ipv4: NET_PIPELINE_GATE_NO_IPV4.load(Ordering::Relaxed),
        net_pipeline_listener_on: NET_PIPELINE_LISTENER_ON.load(Ordering::Relaxed),
        net_pipeline_listener_off: NET_PIPELINE_LISTENER_OFF.load(Ordering::Relaxed),
        net_pipeline_accept_wait_count: NET_PIPELINE_ACCEPT_WAIT_COUNT.load(Ordering::Relaxed),
        net_pipeline_accept_wait_ms_total: NET_PIPELINE_ACCEPT_WAIT_MS_TOTAL
            .load(Ordering::Relaxed),
        net_pipeline_accept_wait_ms_max: NET_PIPELINE_ACCEPT_WAIT_MS_MAX.load(Ordering::Relaxed),
        net_pipeline_accept_arm_gap_count: NET_PIPELINE_ACCEPT_ARM_GAP_COUNT
            .load(Ordering::Relaxed),
        net_pipeline_accept_arm_gap_us_total: NET_PIPELINE_ACCEPT_ARM_GAP_US_TOTAL
            .load(Ordering::Relaxed),
        net_pipeline_accept_arm_gap_us_max: NET_PIPELINE_ACCEPT_ARM_GAP_US_MAX
            .load(Ordering::Relaxed),
        net_pipeline_accept_arm_gap_after_mkdir_count:
            NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_COUNT.load(Ordering::Relaxed),
        net_pipeline_accept_arm_gap_after_mkdir_us_total:
            NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_US_TOTAL.load(Ordering::Relaxed),
        net_pipeline_accept_arm_gap_after_mkdir_us_max:
            NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_US_MAX.load(Ordering::Relaxed),
        wifi_link_rssi_last_dbm: decode_rssi_dbm(WIFI_LINK_RSSI_LAST_DBM.load(Ordering::Relaxed)),
        wifi_link_rssi_min_dbm: decode_rssi_dbm(WIFI_LINK_RSSI_MIN_DBM.load(Ordering::Relaxed)),
        wifi_link_rssi_max_dbm: decode_rssi_dbm(WIFI_LINK_RSSI_MAX_DBM.load(Ordering::Relaxed)),
        wifi_link_rssi_samples: WIFI_LINK_RSSI_SAMPLES.load(Ordering::Relaxed),
        wifi_link_rssi_low_samples: WIFI_LINK_RSSI_LOW_SAMPLES.load(Ordering::Relaxed),
        wifi_link_connected: WIFI_LINK_CONNECTED.load(Ordering::Relaxed),
        wifi_ipv4,
        upload_http_listening: UPLOAD_HTTP_LISTENING.load(Ordering::Relaxed),
        upload_http_ipv4,
    }
}
