use super::*;
pub(super) async fn handle_error_recovery_paths(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    disconnect_reason: u8,
    discovery_reason: bool,
    auth_reason: bool,
    observed_candidates: heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    observed_ap: Option<TargetApCandidate>,
    observed_scan_nonzero: bool,
) {
    if let Some(ap) = observed_ap {
        state.discovery_sweep_exhausted_streak = 0;
        state.zero_discovery_hard_guard_restarts = 0;
        state.force_full_channel_probe_next_scan = false;
        let mut selected_ap = ap;
        let mut forced_rotation = false;
        if disconnect_reason == WIFI_REASON_OTHER && observed_candidates.len() > 1 {
            let rotate_from = state.bssid_hint.unwrap_or(ap.hint.bssid);
            if let Some(next_candidate) = rotate_to_next_candidate(
                &observed_candidates,
                Some(rotate_from),
                &mut state.ap_candidate_idx,
            ) {
                if next_candidate.hint.bssid != rotate_from {
                    selected_ap = next_candidate;
                    forced_rotation = true;
                    diag_reassoc!(
                        "upload_http: reason=other; forcing candidate rotation idx={} channel_hint={} bssid_hint={} count={}",
                        state.ap_candidate_idx,
                        next_candidate.hint.channel,
                        format_bssid(next_candidate.hint.bssid),
                        observed_candidates.len(),
                    );
                }
            }
        }
        let selected_bssid = selected_ap.hint.bssid;
        state.ap_candidate_idx = observed_candidates
            .iter()
            .position(|candidate| candidate.hint.bssid == selected_bssid)
            .unwrap_or(0);
        state.ap_candidates = observed_candidates;
        if disconnect_reason == WIFI_REASON_OTHER && state.other_disconnect_streak >= 2 {
            state.channel_hint = Some(selected_ap.hint.channel);
            state.bssid_hint = None;
            state.auth_method_idx = (state.auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
            if state.other_disconnect_streak >= 2 + WIFI_AUTH_METHODS.len() as u8
                && state.auth_method_idx == 0
            {
                state.channel_hint = None;
                state.channel_probe_idx = 0;
                state.ap_candidates.clear();
                state.ap_candidate_idx = 0;
                diag_reassoc!(
                    "upload_http: reason=other streak={} exhausted auth sweep; forcing full discovery",
                    state.other_disconnect_streak,
                );
            }
            state.config_applied = false;
            state.dhcp_same_candidate_timeout_streak = 0;
            state.discovery_sweep_exhausted_streak = 0;
            state.zero_discovery_hard_guard_restarts = 0;
            state.force_full_channel_probe_next_scan = false;
            telemetry::record_wifi_reassoc_hint_retry(
                state.channel_hint.unwrap_or(selected_ap.hint.channel),
                state.auth_method_idx,
                state.channel_probe_idx,
            );
            diag_reassoc!(
                "upload_http: reason=other streak={}; dropping bssid pin for retry auth={:?} channel_hint={:?}",
                state.other_disconnect_streak,
                WIFI_AUTH_METHODS[state.auth_method_idx],
                state.channel_hint,
            );
            Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
            return;
        }
        if forced_rotation
            || state.channel_hint != Some(selected_ap.hint.channel)
            || state.bssid_hint != Some(selected_ap.hint.bssid)
        {
            state.channel_hint = Some(selected_ap.hint.channel);
            state.bssid_hint = Some(selected_ap.hint.bssid);
            state.auth_method_idx = 0;
            state.config_applied = false;
            state.dhcp_same_candidate_timeout_streak = 0;
            state.discovery_sweep_exhausted_streak = 0;
            state.zero_discovery_hard_guard_restarts = 0;
            state.force_full_channel_probe_next_scan = false;
            telemetry::record_wifi_reassoc_hint_retry(
                selected_ap.hint.channel,
                state.auth_method_idx,
                state.channel_probe_idx,
            );
            diag_reassoc!(
                "upload_http: retrying with candidate idx={} channel_hint={} bssid_hint={} count={}",
                state.ap_candidate_idx,
                selected_ap.hint.channel,
                format_bssid(selected_ap.hint.bssid),
                state.ap_candidates.len(),
            );
            Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
            return;
        }
        diag_reassoc!(
            "upload_http: keeping discovered channel_hint={} bssid_hint={} for next auth attempt (candidate_count={})",
            ap.hint.channel,
            format_bssid(ap.hint.bssid),
            state.ap_candidates.len(),
        );
    }

    if discovery_reason {
        if state.channel_probe_idx < WIFI_CHANNEL_PROBE_SEQUENCE.len() {
            let next_channel = next_probe_channel(&mut state.channel_probe_idx);
            state.channel_hint = Some(next_channel);
            state.bssid_hint = None;
            state.auth_method_idx = 0;
            state.config_applied = false;
            state.dhcp_same_candidate_timeout_streak = 0;
            telemetry::record_wifi_reassoc_channel_probe(next_channel, state.channel_probe_idx);
            diag_reassoc!(
                "upload_http: discovery retry via channel probe channel_hint={} probe_idx={}",
                next_channel,
                state.channel_probe_idx
            );
            Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
            return;
        }
        if !observed_scan_nonzero {
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
                    disconnect_and_stop_with_timeout(controller, "zero_discovery_guard_terminal")
                        .await;
                    telemetry::set_wifi_link_connected(false);
                    telemetry::set_upload_http_listener(false, None);
                    state.hard_recover_watchdog_started_at = None;
                    return;
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
            disconnect_and_stop_with_timeout(
                controller,
                "discovery_sweep_exhausted_driver_restart",
            )
            .await;
            telemetry::set_wifi_link_connected(false);
            telemetry::set_upload_http_listener(false, None);
            if state.hard_recover_watchdog_started_at.is_none() {
                state.hard_recover_watchdog_started_at = Some(Instant::now());
            }
            Timer::after(Duration::from_millis(
                state.runtime_policy.driver_restart_backoff_ms as u64,
            ))
            .await;
            return;
        }
        diag_reassoc!(
            "upload_http: discovery sweep exhausted streak={} max={} hard_guard_restarts={}; clearing hints for full rescan",
            state.discovery_sweep_exhausted_streak,
            state.runtime_policy.full_scan_reset_max,
            state.zero_discovery_hard_guard_restarts,
        );
        Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
        return;
    }

    state.discovery_sweep_exhausted_streak = 0;
    state.zero_discovery_hard_guard_restarts = 0;
    if auth_reason {
        state.auth_method_idx = (state.auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
        state.config_applied = false;
        telemetry::record_wifi_reassoc_auth_rotation(
            state.auth_method_idx,
            state.channel_hint,
            state.channel_probe_idx,
        );
        diag_reassoc!(
            "upload_http: rotating auth on hinted channel auth={:?} channel_hint={:?} bssid_hint={}",
            WIFI_AUTH_METHODS[state.auth_method_idx],
            state.channel_hint,
            format_bssid_opt(state.bssid_hint),
        );
        if state.auth_method_idx == 0 && state.channel_hint.is_some() {
            if let Some(next_candidate) = rotate_to_next_candidate(
                &state.ap_candidates,
                state.bssid_hint,
                &mut state.ap_candidate_idx,
            ) {
                state.channel_hint = Some(next_candidate.hint.channel);
                state.bssid_hint = Some(next_candidate.hint.bssid);
                state.config_applied = false;
                state.dhcp_same_candidate_timeout_streak = 0;
                diag_reassoc!(
                    "upload_http: auth methods exhausted; switching to next candidate idx={} channel_hint={} bssid_hint={}",
                    state.ap_candidate_idx,
                    next_candidate.hint.channel,
                    format_bssid(next_candidate.hint.bssid),
                );
            } else {
                state.channel_hint = None;
                state.bssid_hint = None;
                state.channel_probe_idx = 0;
                state.dhcp_same_candidate_timeout_streak = 0;
                diag_reassoc!(
                    "upload_http: auth methods exhausted on hinted channel; clearing hints for discovery sweep",
                );
            }
        }
        Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
        return;
    }

    state.channel_probe_idx = 0;
    state.channel_hint = None;
    state.bssid_hint = None;
    state.ap_candidates.clear();
    state.ap_candidate_idx = 0;
    state.auth_method_idx = 0;
    state.config_applied = false;
    state.dhcp_same_candidate_timeout_streak = 0;
    diag_reassoc!(
        "upload_http: reconnect fallback reason={} ({}); forcing fresh full scan",
        disconnect_reason,
        disconnect_reason_label(disconnect_reason)
    );
    Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
}
