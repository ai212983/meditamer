use super::*;

pub(super) async fn recover_dhcp_no_ipv4_stall(
    controller: &mut WifiController<'_>,
    state: &mut WifiTaskState,
    stall_elapsed_ms: u32,
    trigger: &'static str,
    reacquire_reason: &'static str,
) -> bool {
    observability::record_wifi_reassoc_disconnect_event(WIFI_REASON_DHCP_NO_IPV4_STALL);
    observability::record_wifi_watchdog_disconnect();
    state.failure_class = NetFailureClass::DhcpNoIpv4;
    state.failure_code = WIFI_REASON_DHCP_NO_IPV4_STALL;
    state.ladder_step = RecoveryLadderStep::RetrySame;
    transition_state(
        &mut state.net_state,
        NetState::Recovering,
        trigger,
        state.started_at,
        state.ladder_step,
        state.net_attempt,
        (state.failure_class, state.failure_code),
    );
    publish_state(
        state.net_state,
        state.ladder_step,
        state.net_attempt,
        state.failure_class,
        state.failure_code,
        state.started_at.elapsed().as_millis() as u32,
    );
    reset_listener_timeout_guard(state);
    if state.dhcp_lease_reacquire_attempts < WIFI_DHCP_LEASE_REACQUIRE_MAX_ATTEMPTS {
        state.dhcp_lease_reacquire_attempts = state.dhcp_lease_reacquire_attempts.saturating_add(1);
        state.config_applied = false;
        diag_reassoc!(
            "upload_http: dhcp/no-ipv4 stall; lease reacquire attempt {}/{} auth={:?} channel_hint={:?} bssid_hint={}",
            state.dhcp_lease_reacquire_attempts,
            WIFI_DHCP_LEASE_REACQUIRE_MAX_ATTEMPTS,
            WIFI_AUTH_METHODS[state.auth_method_idx],
            state.channel_hint,
            format_bssid_opt(state.bssid_hint),
        );
        disconnect_with_timeout(controller, reacquire_reason).await;
        observability::set_wifi_link_connected(false);
        observability::set_upload_http_listener(false, None);
        Timer::after(Duration::from_millis(WIFI_DHCP_LEASE_REACQUIRE_BACKOFF_MS)).await;
        return true;
    }

    state.dhcp_lease_reacquire_attempts = 0;
    let previous_bssid = state.bssid_hint;
    if previous_bssid.is_some() {
        diag_wifi!(
            "upload_http: dhcp/no-ipv4 stall on pinned bssid after {}ms; clearing bssid hint and reconnecting",
            stall_elapsed_ms
        );
    } else {
        diag_wifi!(
            "upload_http: dhcp/no-ipv4 stall after {}ms; reconnecting and retrying scan/auth",
            stall_elapsed_ms
        );
        state.channel_probe_idx = 0;
    }
    if let Some(next_candidate) = rotate_to_next_candidate(
        &state.ap_candidates,
        previous_bssid,
        &mut state.ap_candidate_idx,
    ) {
        state.channel_hint = Some(next_candidate.hint.channel);
        state.bssid_hint = Some(next_candidate.hint.bssid);
        state.auth_method_idx = 0;
        state.config_applied = false;
        if previous_bssid == Some(next_candidate.hint.bssid) {
            state.dhcp_same_candidate_timeout_streak =
                state.dhcp_same_candidate_timeout_streak.saturating_add(1);
        } else {
            state.dhcp_same_candidate_timeout_streak = 0;
        }
        diag_reassoc!(
            "upload_http: dhcp/no-ipv4 stall candidate rotate idx={} channel_hint={} bssid_hint={} same_streak={} candidates={}",
            state.ap_candidate_idx,
            next_candidate.hint.channel,
            format_bssid(next_candidate.hint.bssid),
            state.dhcp_same_candidate_timeout_streak,
            state.ap_candidates.len(),
        );
    } else {
        state.dhcp_same_candidate_timeout_streak =
            state.dhcp_same_candidate_timeout_streak.saturating_add(1);
        state.channel_hint = None;
        state.bssid_hint = None;
        state.auth_method_idx = 0;
        state.config_applied = false;
        state.channel_probe_idx = 0;
        diag_reassoc!(
            "upload_http: dhcp/no-ipv4 stall no candidate available; forcing fresh discovery streak={}",
            state.dhcp_same_candidate_timeout_streak,
        );
    }
    let _ = wifi_disconnect_async(controller).await;
    if state.dhcp_same_candidate_timeout_streak >= WIFI_DHCP_SAME_CANDIDATE_RESTART_STREAK {
        diag_reassoc!(
            "upload_http: dhcp/no-ipv4 stall streak={} reached; forcing wifi stop/start and full rescan",
            state.dhcp_same_candidate_timeout_streak,
        );
        let _ = wifi_stop_async(controller).await;
        state.ap_candidates.clear();
        state.ap_candidate_idx = 0;
        state.channel_hint = None;
        state.bssid_hint = None;
        state.auth_method_idx = 0;
        state.config_applied = false;
        state.channel_probe_idx = 0;
        state.dhcp_same_candidate_timeout_streak = 0;
    }
    observability::set_wifi_link_connected(false);
    observability::set_upload_http_listener(false, None);
    true
}
