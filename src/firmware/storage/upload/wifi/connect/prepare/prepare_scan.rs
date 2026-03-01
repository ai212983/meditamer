use super::*;

pub(super) async fn prepare_scan_candidates(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    active: WifiCredentials,
) -> bool {
    if state.channel_hint.is_none() && state.escalated_auth_sweep_attempts_left == 0 {
        transition_state(
            &mut state.net_state,
            NetState::Scanning,
            "scan_candidates",
            state.started_at,
            state.ladder_step,
            state.net_attempt,
            (state.failure_class, state.failure_code),
        );
        if let Ok(ssid) = core::str::from_utf8(&active.ssid[..active.ssid_len as usize]) {
            let force_full_channel_probe = state.force_full_channel_probe_next_scan;
            let scan_outcome = scan_target_candidates(
                controller,
                ssid,
                state.runtime_policy,
                force_full_channel_probe,
            )
            .await;
            state.force_full_channel_probe_next_scan = false;
            if scan_outcome.hit_nomem {
                state.failure_class = NetFailureClass::Transport;
                state.failure_code = WIFI_REASON_SCAN_NOMEM;
                state.ladder_step = RecoveryLadderStep::DriverRestart;
                transition_state(
                    &mut state.net_state,
                    NetState::Recovering,
                    "scan_nomem",
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
                disconnect_and_stop_with_timeout(controller, "scan_nomem").await;
                telemetry::set_wifi_link_connected(false);
                telemetry::set_upload_http_listener(false, None);
                state.config_applied = false;
                state.channel_hint = None;
                state.bssid_hint = None;
                state.ap_candidates.clear();
                state.ap_candidate_idx = 0;
                state.channel_probe_idx = 0;
                state.auth_method_idx = 0;
                state.dhcp_same_candidate_timeout_streak = 0;
                state.dhcp_lease_reacquire_attempts = 0;
                state.other_disconnect_streak = 0;
                state.discovery_sweep_exhausted_streak = 0;
                state.zero_discovery_hard_guard_restarts = 0;
                state.force_full_channel_probe_next_scan = false;
                if state.hard_recover_watchdog_started_at.is_none() {
                    state.hard_recover_watchdog_started_at = Some(Instant::now());
                }
                Timer::after(Duration::from_millis(WIFI_NOMEM_RECOVERY_BACKOFF_MS)).await;
                return true;
            }
            let scanned_candidates = scan_outcome.candidates;
            if scan_outcome.saw_nonzero_results {
                state.discovery_sweep_exhausted_streak = 0;
                state.zero_discovery_hard_guard_restarts = 0;
            }
            if let Some(candidate) = scanned_candidates.first().copied() {
                state.ap_candidates = scanned_candidates;
                state.ap_candidate_idx = 0;
                state.channel_hint = Some(candidate.hint.channel);
                state.bssid_hint = Some(candidate.hint.bssid);
                state.auth_method_idx = 0;
                state.config_applied = false;
                state.channel_probe_idx = 0;
                state.discovery_sweep_exhausted_streak = 0;
                state.zero_discovery_hard_guard_restarts = 0;
                diag_reassoc!(
                    "upload_http: pre-connect selected candidate idx={} channel_hint={} bssid_hint={} candidate_count={}",
                    state.ap_candidate_idx,
                    candidate.hint.channel,
                    format_bssid(candidate.hint.bssid),
                    state.ap_candidates.len(),
                );
                Timer::after(Duration::from_millis(WIFI_SHORT_SETTLE_MS)).await;
                return true;
            }
        }
    }

    false
}
