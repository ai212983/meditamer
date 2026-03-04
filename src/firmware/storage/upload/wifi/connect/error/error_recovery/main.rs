pub(super) async fn handle_error_recovery_paths(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    disconnect_reason: u8,
    discovery_reason: bool,
    auth_reason: bool,
    observed_candidates: heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    observed_ap: Option<TargetApCandidate>,
    observed_target_candidate: bool,
) {
    if handle_observed_ap_paths(state, disconnect_reason, observed_candidates, observed_ap).await {
        return;
    }

    if handle_discovery_reason_paths(
        controller,
        state,
        disconnect_reason,
        discovery_reason,
        observed_target_candidate,
    )
    .await
    {
        return;
    }

    state.discovery_sweep_exhausted_streak = 0;
    state.zero_discovery_hard_guard_restarts = 0;

    if handle_auth_reason_paths(state, auth_reason).await {
        return;
    }

    handle_reconnect_fallback(state, disconnect_reason).await;
}
