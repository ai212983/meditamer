use super::*;
pub(super) fn state_mem_stage(state: NetState) -> Option<&'static str> {
    match state {
        NetState::Starting => Some("state_starting"),
        NetState::Scanning => Some("state_scanning"),
        NetState::Associating => Some("state_associating"),
        NetState::DhcpWait => Some("state_dhcp_wait"),
        NetState::ListenerWait => Some("state_listener_wait"),
        NetState::Ready => Some("state_ready"),
        NetState::Recovering => Some("state_recovering"),
        NetState::Idle | NetState::Failed => None,
    }
}

pub(super) fn install_wifi_event_logger() {
    WIFI_EVENT_LOGGER_INSTALLED.store(true, Ordering::Relaxed);
}

pub(super) fn disconnect_reason_label(reason: u8) -> &'static str {
    match reason {
        WIFI_REASON_BEACON_TIMEOUT => "beacon_timeout",
        WIFI_REASON_NO_AP_FOUND => "no_ap_found",
        WIFI_REASON_AUTH_FAIL => "auth_fail",
        WIFI_REASON_ASSOC_FAIL => "assoc_fail",
        WIFI_REASON_HANDSHAKE_TIMEOUT => "handshake_timeout",
        WIFI_REASON_CONNECTION_FAIL => "connection_fail",
        WIFI_REASON_NO_AP_FOUND_COMPAT_SECURITY => "no_ap_found_compatible_security",
        WIFI_REASON_NO_AP_FOUND_AUTHMODE_THRESHOLD => "no_ap_found_authmode_threshold",
        WIFI_REASON_NO_AP_FOUND_RSSI_THRESHOLD => "no_ap_found_rssi_threshold",
        WIFI_REASON_CONNECT_LOW_INTERNAL_MEM => "connect_low_internal_mem",
        WIFI_REASON_DHCP_NO_IPV4_STALL => "dhcp_no_ipv4_stall",
        WIFI_REASON_POST_HARD_RECOVER_CONNECT_STALL => "post_hard_recover_connect_stall",
        WIFI_REASON_CONNECT_ATTEMPT_TIMEOUT => "connect_attempt_timeout",
        WIFI_REASON_START_NOMEM => "start_nomem",
        WIFI_REASON_SCAN_NOMEM => "scan_nomem",
        _ => "other",
    }
}

pub(super) fn is_discovery_disconnect_reason(reason: u8) -> bool {
    reason == WIFI_REASON_BEACON_TIMEOUT
        || reason == WIFI_REASON_NO_AP_FOUND
        || reason == WIFI_REASON_NO_AP_FOUND_RSSI_THRESHOLD
}

pub(super) fn is_auth_disconnect_reason(reason: u8) -> bool {
    reason == WIFI_REASON_AUTH_FAIL
        || reason == WIFI_REASON_ASSOC_FAIL
        || reason == WIFI_REASON_HANDSHAKE_TIMEOUT
        || reason == WIFI_REASON_CONNECTION_FAIL
        || reason == WIFI_REASON_NO_AP_FOUND_COMPAT_SECURITY
        || reason == WIFI_REASON_NO_AP_FOUND_AUTHMODE_THRESHOLD
}

pub(super) fn next_probe_channel(index: &mut usize) -> u8 {
    let channel = WIFI_CHANNEL_PROBE_SEQUENCE[*index % WIFI_CHANNEL_PROBE_SEQUENCE.len()];
    *index = index.saturating_add(1);
    channel
}
