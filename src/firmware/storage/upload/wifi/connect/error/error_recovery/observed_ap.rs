async fn handle_observed_ap_paths(
    state: &mut WifiTaskState,
    disconnect_reason: u8,
    observed_candidates: heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    observed_ap: Option<TargetApCandidate>,
) -> bool {
    if let Some(ap) = observed_ap {
        state.discovery_sweep_exhausted_streak = 0;
        state.zero_discovery_hard_guard_restarts = 0;
        state.force_full_channel_probe_next_scan = false;
        let mut selected_ap = ap;
        let mut forced_rotation = false;
        let hinted_candidate = state.bssid_hint.and_then(|hinted_bssid| {
            observed_candidates
                .iter()
                .position(|candidate| candidate.hint.bssid == hinted_bssid)
                .map(|idx| (idx, observed_candidates[idx]))
        });
        if disconnect_reason == WIFI_REASON_OTHER && observed_candidates.len() > 1 {
            if let Some((hinted_idx, hinted_ap)) = hinted_candidate {
                selected_ap = hinted_ap;
                state.ap_candidate_idx = hinted_idx;
                diag_reassoc!(
                    "upload_http: reason=other preserving hinted candidate idx={} channel_hint={} bssid_hint={} count={}",
                    state.ap_candidate_idx,
                    hinted_ap.hint.channel,
                    format_bssid(hinted_ap.hint.bssid),
                    observed_candidates.len(),
                );
            } else {
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
        }
        let selected_bssid = selected_ap.hint.bssid;
        state.ap_candidate_idx = observed_candidates
            .iter()
            .position(|candidate| candidate.hint.bssid == selected_bssid)
            .unwrap_or(0);
        state.ap_candidates = observed_candidates;
        if is_auth_disconnect_reason(disconnect_reason) {
            if let Some(hinted_bssid) = state.bssid_hint {
                if let Some(hinted_idx) = state
                    .ap_candidates
                    .iter()
                    .position(|candidate| candidate.hint.bssid == hinted_bssid)
                {
                    let hinted_candidate = state.ap_candidates[hinted_idx];
                    state.ap_candidate_idx = hinted_idx;
                    state.channel_hint = Some(hinted_candidate.hint.channel);
                    diag_reassoc!(
                        "upload_http: auth-reject preserving hinted candidate idx={} channel_hint={} bssid_hint={} selected_bssid={} count={}",
                        state.ap_candidate_idx,
                        hinted_candidate.hint.channel,
                        format_bssid(hinted_candidate.hint.bssid),
                        format_bssid(selected_ap.hint.bssid),
                        state.ap_candidates.len(),
                    );
                    return false;
                }
            }
        }
        if disconnect_reason == WIFI_REASON_OTHER && state.other_disconnect_streak >= 2 {
            if let Some((hinted_idx, hinted_ap)) = hinted_candidate {
                state.ap_candidate_idx = hinted_idx;
                state.channel_hint = Some(hinted_ap.hint.channel);
                state.bssid_hint = Some(hinted_ap.hint.bssid);
                state.auth_method_idx = (state.auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
                state.config_applied = false;
                state.dhcp_same_candidate_timeout_streak = 0;
                state.discovery_sweep_exhausted_streak = 0;
                state.zero_discovery_hard_guard_restarts = 0;
                state.force_full_channel_probe_next_scan = false;
                telemetry::record_wifi_reassoc_auth_rotation(
                    state.auth_method_idx,
                    state.channel_hint,
                    state.channel_probe_idx,
                );
                if state.auth_method_idx == 0 {
                    if let Some(next_candidate) = rotate_to_next_candidate(
                        &state.ap_candidates,
                        Some(hinted_ap.hint.bssid),
                        &mut state.ap_candidate_idx,
                    ) {
                        state.channel_hint = Some(next_candidate.hint.channel);
                        state.bssid_hint = Some(next_candidate.hint.bssid);
                        diag_reassoc!(
                            "upload_http: reason=other pinned auth sweep exhausted; switching to next candidate idx={} channel_hint={} bssid_hint={}",
                            state.ap_candidate_idx,
                            next_candidate.hint.channel,
                            format_bssid(next_candidate.hint.bssid),
                        );
                    } else {
                        state.channel_hint = None;
                        state.bssid_hint = None;
                        state.channel_probe_idx = 0;
                        diag_reassoc!(
                            "upload_http: reason=other pinned auth sweep exhausted; clearing hints for discovery",
                        );
                    }
                } else {
                    diag_reassoc!(
                        "upload_http: reason=other streak={} preserving hinted candidate idx={} retry auth={:?} channel_hint={} bssid_hint={}",
                        state.other_disconnect_streak,
                        state.ap_candidate_idx,
                        WIFI_AUTH_METHODS[state.auth_method_idx],
                        hinted_ap.hint.channel,
                        format_bssid(hinted_ap.hint.bssid),
                    );
                }
                Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                return true;
            }

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
            return true;
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
            return true;
        }
        diag_reassoc!(
            "upload_http: keeping discovered channel_hint={} bssid_hint={} for next auth attempt (candidate_count={})",
            ap.hint.channel,
            format_bssid(ap.hint.bssid),
            state.ap_candidates.len(),
        );
    }

    false
}
