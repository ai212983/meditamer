use super::super::*;

pub(super) async fn handle_reconnect_fallback(state: &mut WifiTaskState, disconnect_reason: u8) {
    state.channel_probe_idx = 0;
    state.channel_hint = None;
    state.bssid_hint = None;
    state.ap_candidates.clear();
    state.ap_candidate_idx = 0;
    state.auth_method_idx = 0;
    state.config_applied = false;
    state.dhcp_same_candidate_timeout_streak = 0;
    diag_reassoc!(
        "upload_http: reconnect fallback reason={} ({}); forcing fresh full scan",
        disconnect_reason,
        disconnect_reason_label(disconnect_reason)
    );
    Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
}
