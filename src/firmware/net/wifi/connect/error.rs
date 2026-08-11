use super::*;

mod low_mem;

use super::retry::{handle_error_recovery_paths, ErrorRecoveryObservation};
use low_mem::recover_connect_err_low_internal_mem;

const CONNECT_ERR_LOW_INTERNAL_FREE_BYTES: usize = 2_048;

pub(super) async fn handle_connect_error(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    err: WifiError,
    connect_started_at: Instant,
) {
    let disconnect_reason = WIFI_LAST_DISCONNECT_REASON.swap(0, Ordering::Relaxed);
    state.dhcp_lease_reacquire_attempts = 0;
    if disconnect_reason == WIFI_REASON_OTHER {
        state.other_disconnect_streak = state.other_disconnect_streak.saturating_add(1);
    } else if disconnect_reason != 0 {
        state.other_disconnect_streak = 0;
    }
    telemetry::record_wifi_connect_failure(disconnect_reason);
    telemetry::record_wifi_reassoc_connect_failure_detail(
        disconnect_reason,
        elapsed_ms_u32(connect_started_at),
    );
    state.failure_class = if is_auth_disconnect_reason(disconnect_reason) {
        NetFailureClass::AuthReject
    } else if is_discovery_disconnect_reason(disconnect_reason) {
        NetFailureClass::DiscoveryEmpty
    } else {
        NetFailureClass::ConnectTimeout
    };
    state.failure_code = disconnect_reason;
    state.ladder_step = RecoveryLadderStep::RetrySame;
    transition_state(
        &mut state.net_state,
        NetState::Recovering,
        "connect_err",
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
    telemetry::set_upload_http_listener(false, None);
    disconnect_with_timeout(controller, "connect_err_presolve").await;
    let discovery_reason = is_discovery_disconnect_reason(disconnect_reason);
    let auth_reason = is_auth_disconnect_reason(disconnect_reason);
    let escalated_scan_active = state.escalated_auth_sweep_attempts_left > 0;
    let should_scan = escalated_scan_active
        || discovery_reason
        || state.channel_hint.is_none()
        || state.channel_probe_idx.is_multiple_of(4);
    let mut observed_candidates = heapless::Vec::<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>::new();
    let mut observed_ap = None;
    let mut observed_scan_nomem = false;
    let mut observed_scan_nonzero = false;
    let mut observed_target_candidate = false;
    if should_scan {
        let mut target_ssid = heapless::String::<WIFI_SSID_MAX>::new();
        if let Some(credentials) = state.credentials {
            if let Ok(ssid) =
                core::str::from_utf8(&credentials.ssid[..credentials.ssid_len as usize])
            {
                let _ = target_ssid.push_str(ssid);
            }
        }
        if !target_ssid.is_empty() {
            let force_full_channel_probe = state.force_full_channel_probe_next_scan;
            let scan_outcome = scan_target_candidates(
                controller,
                target_ssid.as_str(),
                state.runtime_policy,
                force_full_channel_probe,
            )
            .await;
            state.force_full_channel_probe_next_scan = false;
            observed_scan_nomem = scan_outcome.hit_nomem;
            observed_scan_nonzero = scan_outcome.saw_nonzero_results;
            observed_target_candidate = scan_outcome.saw_target_candidate;
            state.note_hard_recover_scan_completion(
                "connect_err_scan",
                observed_scan_nonzero,
                observed_target_candidate,
            );
            observed_candidates = scan_outcome.candidates;
            observed_ap = observed_candidates.first().copied();
        }
    }
    let reason_other_preserve_hinted_candidate = disconnect_reason == WIFI_REASON_OTHER
        && state.bssid_hint.is_some_and(|hinted_bssid| {
            observed_candidates
                .iter()
                .any(|candidate| candidate.hint.bssid == hinted_bssid)
        });
    let auth_method = WIFI_AUTH_METHODS[state.auth_method_idx];
    diag_reassoc!(
        "upload_http: wifi connect err={:?} auth={:?} channel_hint={:?} bssid_hint={} observed_channel={:?} observed_bssid={} reason={} (0x{:02x} {}) discovery_reason={} should_scan={} scan_nomem={} scan_any_seen={} scan_target_seen={} probe_idx={}",
        err,
        auth_method,
        state.channel_hint,
        format_bssid_opt(state.bssid_hint),
        observed_ap.map(|ap| ap.hint),
        format_bssid_opt(observed_ap.map(|ap| ap.hint.bssid)),
        disconnect_reason,
        disconnect_reason,
        disconnect_reason_label(disconnect_reason),
        discovery_reason,
        should_scan,
        observed_scan_nomem,
        observed_scan_nonzero,
        observed_target_candidate,
        state.channel_probe_idx,
    );
    if observed_target_candidate {
        state.discovery_sweep_exhausted_streak = 0;
        state.zero_discovery_hard_guard_restarts = 0;
        state.force_full_channel_probe_next_scan = false;
    }
    if observed_scan_nomem {
        state.failure_class = NetFailureClass::Transport;
        state.failure_code = WIFI_REASON_SCAN_NOMEM;
        state.ladder_step = RecoveryLadderStep::DriverRestart;
        transition_state(
            &mut state.net_state,
            NetState::Recovering,
            "connect_err_scan_nomem",
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
        disconnect_and_stop_with_timeout(controller, "connect_err_scan_nomem").await;
        telemetry::set_wifi_link_connected(false);
        telemetry::set_upload_http_listener(false, None);
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
        state.start_hard_recover_watchdog("connect_err_scan_nomem");
        Timer::after(Duration::from_millis(WIFI_NOMEM_RECOVERY_BACKOFF_MS)).await;
        return;
    }
    let internal_free_bytes = psram::allocator_memory_snapshot().free_internal_bytes;
    if internal_free_bytes > 0
        && internal_free_bytes <= CONNECT_ERR_LOW_INTERNAL_FREE_BYTES
        && (disconnect_reason == WIFI_REASON_OTHER || discovery_reason)
    {
        recover_connect_err_low_internal_mem(
            controller,
            state,
            disconnect_reason,
            internal_free_bytes,
            CONNECT_ERR_LOW_INTERNAL_FREE_BYTES,
        )
        .await;
        return;
    }
    if escalated_scan_active {
        state.channel_probe_idx = 0;
        state.channel_hint = None;
        state.bssid_hint = None;
        state.ap_candidates.clear();
        state.ap_candidate_idx = 0;
        state.config_applied = false;
        state.dhcp_same_candidate_timeout_streak = 0;
        if auth_reason {
            state.auth_method_idx = (state.auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
        } else {
            state.auth_method_idx = 0;
        }
        state.escalated_auth_sweep_attempts_left =
            state.escalated_auth_sweep_attempts_left.saturating_sub(1);
        diag_reassoc!(
            "upload_http: post-hard-recover-escalated-scan retry attempts_left={} auth={:?} reason={} ({})",
            state.escalated_auth_sweep_attempts_left,
            WIFI_AUTH_METHODS[state.auth_method_idx],
            disconnect_reason,
            disconnect_reason_label(disconnect_reason),
        );
        Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
        return;
    }

    if disconnect_reason == WIFI_REASON_OTHER
        && state.other_disconnect_streak >= WIFI_REASON_OTHER_HARD_RECOVER_STREAK
        && !reason_other_preserve_hinted_candidate
    {
        state.channel_probe_idx = 0;
        state.channel_hint = None;
        state.bssid_hint = None;
        state.ap_candidates.clear();
        state.ap_candidate_idx = 0;
        state.auth_method_idx = 0;
        state.config_applied = false;
        state.dhcp_same_candidate_timeout_streak = 0;
        state.other_disconnect_streak = 0;
        state.discovery_sweep_exhausted_streak = 0;
        state.zero_discovery_hard_guard_restarts = 0;
        state.force_full_channel_probe_next_scan = false;
        state.start_hard_recover_watchdog("connect_err_reason_other_hard_recover");
        diag_reassoc!(
            "upload_http: connect reason=other streak reached {}; forcing hard wifi recovery (stop/start + full discovery reset)",
            WIFI_REASON_OTHER_HARD_RECOVER_STREAK
        );
        Timer::after(Duration::from_millis(
            state.runtime_policy.driver_restart_backoff_ms as u64,
        ))
        .await;
        return;
    }

    handle_error_recovery_paths(
        controller,
        state,
        ErrorRecoveryObservation {
            disconnect_reason,
            discovery_reason,
            auth_reason,
            candidates: observed_candidates,
            ap: observed_ap,
            target_candidate: observed_target_candidate,
        },
    )
    .await;
}
