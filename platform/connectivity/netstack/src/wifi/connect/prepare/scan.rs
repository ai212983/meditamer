use super::*;

async fn handle_preconnect_zero_discovery(
    controller: &mut WifiController<'_>,
    state: &mut WifiTaskState,
) {
    state.discovery_sweep_exhausted_streak =
        state.discovery_sweep_exhausted_streak.saturating_add(1);
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
    state.failure_class = NetFailureClass::DiscoveryEmpty;
    state.failure_code = WIFI_REASON_NO_AP_FOUND;

    if state.discovery_sweep_exhausted_streak >= state.runtime_policy.full_scan_reset_max {
        let hard_guard_trip =
            state.discovery_sweep_exhausted_streak >= WIFI_ZERO_DISCOVERY_HARD_GUARD_STREAK;
        if hard_guard_trip {
            if state.zero_discovery_hard_guard_restarts
                >= WIFI_ZERO_DISCOVERY_HARD_GUARD_MAX_RESTARTS
            {
                state.ladder_step = RecoveryLadderStep::TerminalFail;
                state.terminal_fail_latched = true;
                transition_state(
                    &mut state.net_state,
                    NetState::Failed,
                    "scan_zero_discovery_guard_terminal",
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
                    "upload_http: pre-connect zero-discovery hard guard terminal: sweep_streak={} restart_retries={} max_retries={}",
                    state.discovery_sweep_exhausted_streak,
                    state.zero_discovery_hard_guard_restarts,
                    WIFI_ZERO_DISCOVERY_HARD_GUARD_MAX_RESTARTS,
                );
                disconnect_and_stop_with_timeout(controller, "scan_zero_discovery_guard_terminal")
                    .await;
                telemetry::set_wifi_link_connected(false);
                telemetry::set_upload_http_listener(false, None);
                state.clear_hard_recover_watchdog("scan_zero_discovery_guard_terminal");
                return;
            }
            state.zero_discovery_hard_guard_restarts =
                state.zero_discovery_hard_guard_restarts.saturating_add(1);
            state.force_full_channel_probe_next_scan = true;
        }
        state.ladder_step = RecoveryLadderStep::DriverRestart;
        transition_state(
            &mut state.net_state,
            NetState::Recovering,
            "scan_zero_discovery_driver_restart",
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
            "upload_http: pre-connect zero-discovery streak={} max={} hard_guard_trip={} hard_guard_restarts={} forcing hard wifi recovery (stop/start)",
            state.discovery_sweep_exhausted_streak,
            state.runtime_policy.full_scan_reset_max,
            hard_guard_trip,
            state.zero_discovery_hard_guard_restarts,
        );
        disconnect_and_stop_with_timeout(controller, "scan_zero_discovery_driver_restart").await;
        telemetry::set_wifi_link_connected(false);
        telemetry::set_upload_http_listener(false, None);
        state.start_hard_recover_watchdog("scan_zero_discovery_driver_restart");
        Timer::after(Duration::from_millis(
            state.runtime_policy.driver_restart_backoff_ms as u64,
        ))
        .await;
        return;
    }

    state.ladder_step = RecoveryLadderStep::RetrySame;
    transition_state(
        &mut state.net_state,
        NetState::Recovering,
        "scan_zero_discovery_retry",
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
        "upload_http: pre-connect zero-discovery streak={} max={}; retrying full scan without connect attempt",
        state.discovery_sweep_exhausted_streak,
        state.runtime_policy.full_scan_reset_max,
    );
    Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
}

pub(super) async fn prepare_scan_candidates(
    controller: &mut WifiController<'_>,
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
            state.note_hard_recover_scan_completion(
                "prepare_scan_candidates",
                scan_outcome.saw_nonzero_results,
                scan_outcome.saw_target_candidate,
            );
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
                state.start_hard_recover_watchdog("scan_nomem");
                Timer::after(Duration::from_millis(WIFI_NOMEM_RECOVERY_BACKOFF_MS)).await;
                return true;
            }
            let scanned_candidates = scan_outcome.candidates;
            if scan_outcome.saw_target_candidate {
                state.discovery_sweep_exhausted_streak = 0;
                state.zero_discovery_hard_guard_restarts = 0;
            } else if !scan_outcome.saw_nonzero_results {
                handle_preconnect_zero_discovery(controller, state).await;
                return true;
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
