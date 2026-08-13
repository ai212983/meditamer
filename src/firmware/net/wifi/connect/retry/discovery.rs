use super::super::*;

pub(super) async fn handle_discovery_reason_paths(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    disconnect_reason: u8,
    discovery_reason: bool,
    observed_target_candidate: bool,
) -> bool {
    if !discovery_reason {
        return false;
    }

    if state.channel_probe_idx < WIFI_CHANNEL_PROBE_SEQUENCE.len() {
        let next_channel = next_probe_channel(&mut state.channel_probe_idx);
        state.channel_hint = Some(next_channel);
        state.bssid_hint = None;
        state.auth_method_idx = 0;
        state.config_applied = false;
        state.dhcp_same_candidate_timeout_streak = 0;
        observability::record_wifi_reassoc_channel_probe(next_channel, state.channel_probe_idx);
        diag_reassoc!(
            "upload_http: discovery retry via channel probe channel_hint={} probe_idx={}",
            next_channel,
            state.channel_probe_idx
        );
        Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
        return true;
    }
    if !observed_target_candidate {
        state.discovery_sweep_exhausted_streak =
            state.discovery_sweep_exhausted_streak.saturating_add(1);
    } else {
        state.discovery_sweep_exhausted_streak = 0;
    }
    state.channel_probe_idx = 0;
    state.channel_hint = None;
    state.bssid_hint = None;
    state.ap_candidates.clear();
    state.ap_candidate_idx = 0;
    state.auth_method_idx = 0;
    state.config_applied = false;
    state.dhcp_same_candidate_timeout_streak = 0;
    if state.discovery_sweep_exhausted_streak >= state.runtime_policy.full_scan_reset_max {
        let hard_guard_trip =
            state.discovery_sweep_exhausted_streak >= WIFI_ZERO_DISCOVERY_HARD_GUARD_STREAK;
        if hard_guard_trip {
            if state.zero_discovery_hard_guard_restarts
                >= WIFI_ZERO_DISCOVERY_HARD_GUARD_MAX_RESTARTS
            {
                state.ladder_step = RecoveryLadderStep::TerminalFail;
                state.failure_class = NetFailureClass::DiscoveryEmpty;
                state.failure_code = WIFI_REASON_NO_AP_FOUND;
                state.terminal_fail_latched = true;
                transition_state(
                    &mut state.net_state,
                    NetState::Failed,
                    "zero_discovery_guard_terminal",
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
                    "upload_http: zero-discovery hard guard terminal: sweep_streak={} restart_retries={} max_retries={}",
                    state.discovery_sweep_exhausted_streak,
                    state.zero_discovery_hard_guard_restarts,
                    WIFI_ZERO_DISCOVERY_HARD_GUARD_MAX_RESTARTS,
                );
                disconnect_and_stop_with_timeout(controller, "zero_discovery_guard_terminal").await;
                observability::set_wifi_link_connected(false);
                observability::set_upload_http_listener(false, None);
                state.clear_hard_recover_watchdog("zero_discovery_guard_terminal");
                maybe_software_reset_on_zero_discovery_terminal(
                    "zero_discovery_guard_terminal",
                    state.discovery_sweep_exhausted_streak,
                    state.zero_discovery_hard_guard_restarts,
                )
                .await;
                return true;
            }
            state.zero_discovery_hard_guard_restarts =
                state.zero_discovery_hard_guard_restarts.saturating_add(1);
            state.force_full_channel_probe_next_scan = true;
        }
        state.ladder_step = RecoveryLadderStep::DriverRestart;
        state.failure_class = NetFailureClass::DiscoveryEmpty;
        state.failure_code = disconnect_reason.max(WIFI_REASON_NO_AP_FOUND);
        transition_state(
            &mut state.net_state,
            NetState::Recovering,
            "discovery_sweep_exhausted_driver_restart",
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
            "upload_http: discovery sweep exhausted streak={} max={} hard_guard_trip={} hard_guard_restarts={} forcing hard wifi recovery (stop/start)",
            state.discovery_sweep_exhausted_streak,
            state.runtime_policy.full_scan_reset_max,
            hard_guard_trip,
            state.zero_discovery_hard_guard_restarts,
        );
        disconnect_and_stop_with_timeout(controller, "discovery_sweep_exhausted_driver_restart")
            .await;
        observability::set_wifi_link_connected(false);
        observability::set_upload_http_listener(false, None);
        maybe_software_reset_on_zero_discovery_hard_guard(
            "discovery_sweep_exhausted_driver_restart",
            hard_guard_trip,
            state.discovery_sweep_exhausted_streak,
            state.zero_discovery_hard_guard_restarts,
        )
        .await;
        state.start_hard_recover_watchdog("discovery_sweep_exhausted_driver_restart");
        Timer::after(Duration::from_millis(
            state.runtime_policy.driver_restart_backoff_ms as u64,
        ))
        .await;
        return true;
    }
    diag_reassoc!(
        "upload_http: discovery sweep exhausted streak={} max={} hard_guard_restarts={}; clearing hints for full rescan",
        state.discovery_sweep_exhausted_streak,
        state.runtime_policy.full_scan_reset_max,
        state.zero_discovery_hard_guard_restarts,
    );
    Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
    true
}
