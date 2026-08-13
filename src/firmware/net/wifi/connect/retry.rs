//! Deciding what to retry after a failed connect attempt.
//!
//! One module per reason class -- a better observed AP, a discovery-side
//! reason, an auth-side reason -- with a plain reconnect as the fallback.

mod auth;
mod discovery;
mod fallback;
mod observed_ap;

use super::*;
use auth::handle_auth_reason_paths;
use discovery::handle_discovery_reason_paths;
use fallback::handle_reconnect_fallback;
use observed_ap::{handle_observed_ap_paths, ObservedApRecoveryInput};

pub(in crate::firmware::net::wifi::connect) struct ErrorRecoveryObservation {
    pub(in crate::firmware::net::wifi::connect) disconnect_reason: u8,
    pub(in crate::firmware::net::wifi::connect) discovery_reason: bool,
    pub(in crate::firmware::net::wifi::connect) auth_reason: bool,
    pub(in crate::firmware::net::wifi::connect) candidates:
        heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    pub(in crate::firmware::net::wifi::connect) ap: Option<TargetApCandidate>,
    pub(in crate::firmware::net::wifi::connect) target_candidate: bool,
}

pub(in crate::firmware::net::wifi::connect) async fn handle_error_recovery_paths(
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
