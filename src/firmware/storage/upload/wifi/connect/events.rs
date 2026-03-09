use super::blob_state_diag::log_scan_done_list_diag;
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
    if WIFI_EVENT_LOGGER_INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }

    event::StaDisconnected::update_handler(|event| {
        let reason = event.reason();
        WIFI_LAST_DISCONNECT_REASON.store(reason, Ordering::Relaxed);
        WIFI_DISCONNECTED_EVENT.store(true, Ordering::Relaxed);
        if cfg!(debug_assertions) {
            diag_reassoc!(
                "upload_http: event sta_disconnected reason={} ({}) rssi={}",
                reason,
                disconnect_reason_label(reason),
                event.rssi()
            );
        }
    });

    if !cfg!(debug_assertions) {
        return;
    }

    event::WifiReady::update_handler(|_| {
        diag_reassoc!("upload_http: event wifi_ready");
    });

    event::StaStart::update_handler(|_| {
        WIFI_LAST_STA_START_AT_MS.store(monotonic_now_ms_u32(), Ordering::Relaxed);
        diag_reassoc!("upload_http: event sta_start");
    });

    event::StaStop::update_handler(|_| {
        WIFI_LAST_STA_STOP_AT_MS.store(monotonic_now_ms_u32(), Ordering::Relaxed);
        diag_reassoc!("upload_http: event sta_stop");
    });

    event::ScanDone::update_handler(|event| {
        WIFI_LAST_SCAN_DONE_AT_MS.store(monotonic_now_ms_u32(), Ordering::Relaxed);
        WIFI_LAST_SCAN_DONE_COUNT.store(u32::from(event.number()), Ordering::Relaxed);
        WIFI_LAST_SCAN_DONE_ID.store(u32::from(event.id()), Ordering::Relaxed);
        WIFI_LAST_SCAN_DONE_STATUS.store(event.status(), Ordering::Relaxed);
        log_scan_done_list_diag(
            u32::from(event.status()),
            u32::from(event.number()),
            u32::from(event.id()),
        );
        maybe_end_first_start_idf_log_diag("scan_done");
        diag_reassoc!(
            "upload_http: event scan_done status={} count={} scan_id={}",
            event.status(),
            event.number(),
            event.id()
        );
    });

    event::StaAuthmodeChange::update_handler(|event| {
        diag_reassoc!(
            "upload_http: event sta_authmode_change old_mode={} new_mode={}",
            event.old_mode(),
            event.new_mode(),
        );
    });

    event::StaBeaconTimeout::update_handler(|_| {
        diag_reassoc!("upload_http: event sta_beacon_timeout");
    });

    event::StaConnected::update_handler(|event| {
        let ssid_len = (event.ssid_len() as usize).min(event.ssid().len());
        let ssid = core::str::from_utf8(&event.ssid()[..ssid_len]).unwrap_or("<non_utf8>");
        let bssid = event.bssid();
        diag_reassoc!(
            "upload_http: event sta_connected ssid={} channel={} authmode={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            ssid,
            event.channel(),
            event.authmode(),
            bssid.first().copied().unwrap_or(0),
            bssid.get(1).copied().unwrap_or(0),
            bssid.get(2).copied().unwrap_or(0),
            bssid.get(3).copied().unwrap_or(0),
            bssid.get(4).copied().unwrap_or(0),
            bssid.get(5).copied().unwrap_or(0),
        );
    });
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
