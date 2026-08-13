use super::*;
pub(super) async fn handle_connect_timeout(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    connect_started_at: Instant,
) {
    state.dhcp_lease_reacquire_attempts = 0;
    observability::record_wifi_connect_failure(WIFI_REASON_CONNECT_ATTEMPT_TIMEOUT);
    observability::record_wifi_reassoc_connect_failure_detail(
        WIFI_REASON_CONNECT_ATTEMPT_TIMEOUT,
        elapsed_ms_u32(connect_started_at),
    );
    state.failure_class = NetFailureClass::ConnectTimeout;
    state.failure_code = WIFI_REASON_CONNECT_ATTEMPT_TIMEOUT;
    state.ladder_step = RecoveryLadderStep::DriverRestart;
    transition_state(
        &mut state.net_state,
        NetState::Recovering,
        "connect_timeout",
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
    observability::set_wifi_link_connected(false);
    observability::set_upload_http_listener(false, None);
    disconnect_and_stop_with_timeout(controller, "connect_timeout").await;
    state.config_applied = false;
    state.channel_hint = None;
    state.bssid_hint = None;
    state.ap_candidates.clear();
    state.ap_candidate_idx = 0;
    state.channel_probe_idx = 0;
    state.auth_method_idx = 0;
    state.dhcp_same_candidate_timeout_streak = 0;
    state.other_disconnect_streak = 0;
    state.start_hard_recover_watchdog("connect_timeout");
    diag_reassoc!(
        "upload_http: wifi connect timeout after {}ms; forcing driver restart and full discovery reset",
        state.runtime_policy.connect_timeout_ms
    );
    Timer::after(Duration::from_millis(
        state.runtime_policy.driver_restart_backoff_ms as u64,
    ))
    .await;
}
