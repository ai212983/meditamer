use super::*;

pub(super) async fn recover_connect_err_low_internal_mem(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    disconnect_reason: u8,
    internal_free_bytes: usize,
    low_internal_free_bytes_threshold: usize,
) {
    state.failure_class = NetFailureClass::Transport;
    state.failure_code = WIFI_REASON_CONNECT_LOW_INTERNAL_MEM;
    state.ladder_step = RecoveryLadderStep::DriverRestart;
    transition_state(
        &mut state.net_state,
        NetState::Recovering,
        "connect_err_low_internal_mem",
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
    diag_reassoc!(
        "upload_http: connect_err low_internal_mem free={} threshold={} reason={} ({}) -> forcing hard wifi recovery",
        internal_free_bytes,
        low_internal_free_bytes_threshold,
        disconnect_reason,
        disconnect_reason_label(disconnect_reason),
    );
    disconnect_and_stop_with_timeout(controller, "connect_err_low_internal_mem").await;
    observability::set_wifi_link_connected(false);
    observability::set_upload_http_listener(false, None);
    state.channel_probe_idx = 0;
    state.channel_hint = None;
    state.bssid_hint = None;
    state.ap_candidates.clear();
    state.ap_candidate_idx = 0;
    state.auth_method_idx = 0;
    state.config_applied = false;
    state.dhcp_same_candidate_timeout_streak = 0;
    state.dhcp_lease_reacquire_attempts = 0;
    state.other_disconnect_streak = 0;
    state.discovery_sweep_exhausted_streak = 0;
    state.zero_discovery_hard_guard_restarts = 0;
    state.force_full_channel_probe_next_scan = false;
    state.start_hard_recover_watchdog("connect_err_low_internal_mem");
    Timer::after(Duration::from_millis(WIFI_NOMEM_RECOVERY_BACKOFF_MS)).await;
}
