pub(super) struct ErrorRecoveryObservation {
    pub(super) disconnect_reason: u8,
    pub(super) discovery_reason: bool,
    pub(super) auth_reason: bool,
    pub(super) candidates: heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    pub(super) ap: Option<TargetApCandidate>,
    pub(super) target_candidate: bool,
}

pub(super) async fn handle_error_recovery_paths(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    observation: ErrorRecoveryObservation,
) {
    let ErrorRecoveryObservation {
        disconnect_reason,
        discovery_reason,
        auth_reason,
        candidates,
        ap,
        target_candidate,
    } = observation;
    if handle_observed_ap_paths(
        state,
        ObservedApRecoveryInput {
            disconnect_reason,
            candidates,
            ap,
        },
    )
    .await
    {
        return;
    }

    if handle_discovery_reason_paths(
        controller,
        state,
        disconnect_reason,
        discovery_reason,
        target_candidate,
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
