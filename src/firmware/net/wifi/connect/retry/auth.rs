use super::super::*;

pub(super) async fn handle_auth_reason_paths(state: &mut WifiTaskState, auth_reason: bool) -> bool {
    if !auth_reason {
        return false;
    }

    state.auth_method_idx = (state.auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
    state.config_applied = false;
    observability::record_wifi_reassoc_auth_rotation(
        state.auth_method_idx,
        state.channel_hint,
        state.channel_probe_idx,
    );
    diag_reassoc!(
        "upload_http: rotating auth on hinted channel auth={:?} channel_hint={:?} bssid_hint={}",
        WIFI_AUTH_METHODS[state.auth_method_idx],
        state.channel_hint,
        format_bssid_opt(state.bssid_hint),
    );
    if state.auth_method_idx == 0 && state.channel_hint.is_some() {
        if let Some(next_candidate) = rotate_to_next_candidate(
            &state.ap_candidates,
            state.bssid_hint,
            &mut state.ap_candidate_idx,
        ) {
            state.channel_hint = Some(next_candidate.hint.channel);
            state.bssid_hint = Some(next_candidate.hint.bssid);
            state.config_applied = false;
            state.dhcp_same_candidate_timeout_streak = 0;
            diag_reassoc!(
                "upload_http: auth methods exhausted; switching to next candidate idx={} channel_hint={} bssid_hint={}",
                state.ap_candidate_idx,
                next_candidate.hint.channel,
                format_bssid(next_candidate.hint.bssid),
            );
        } else {
            state.channel_hint = None;
            state.bssid_hint = None;
            state.channel_probe_idx = 0;
            state.dhcp_same_candidate_timeout_streak = 0;
            diag_reassoc!(
                "upload_http: auth methods exhausted on hinted channel; clearing hints for discovery sweep",
            );
        }
    }
    Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
    true
}
