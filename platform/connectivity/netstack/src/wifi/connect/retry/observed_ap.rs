use super::super::*;

pub(super) struct ObservedApRecoveryInput {
    pub(super) disconnect_reason: u8,
    pub(super) candidates: heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    pub(super) ap: Option<TargetApCandidate>,
}

#[derive(Clone, Copy)]
struct CandidateSelection {
    selected: TargetApCandidate,
    hinted: Option<(usize, TargetApCandidate)>,
    forced_rotation: bool,
}

fn reset_observed_discovery_state(state: &mut WifiTaskState) {
    state.discovery_sweep_exhausted_streak = 0;
    state.zero_discovery_hard_guard_restarts = 0;
    state.force_full_channel_probe_next_scan = false;
}

fn reset_candidate_retry_state(state: &mut WifiTaskState) {
    state.config_applied = false;
    state.dhcp_same_candidate_timeout_streak = 0;
    reset_observed_discovery_state(state);
}

fn select_candidate(
    state: &mut WifiTaskState,
    disconnect_reason: u8,
    candidates: &[TargetApCandidate],
    observed_ap: TargetApCandidate,
) -> CandidateSelection {
    let hinted = state.bssid_hint.and_then(|hinted_bssid| {
        candidates
            .iter()
            .position(|candidate| candidate.hint.bssid == hinted_bssid)
            .map(|idx| (idx, candidates[idx]))
    });
    let mut selection = CandidateSelection {
        selected: observed_ap,
        hinted,
        forced_rotation: false,
    };

    if disconnect_reason != WIFI_REASON_OTHER || candidates.len() <= 1 {
        return selection;
    }

    if let Some((hinted_idx, hinted_ap)) = hinted {
        selection.selected = hinted_ap;
        state.ap_candidate_idx = hinted_idx;
        diag_reassoc!(
            "upload_http: reason=other preserving hinted candidate idx={} channel_hint={} bssid_hint={} count={}",
            state.ap_candidate_idx,
            hinted_ap.hint.channel,
            format_bssid(hinted_ap.hint.bssid),
            candidates.len(),
        );
        return selection;
    }

    let rotate_from = state.bssid_hint.unwrap_or(observed_ap.hint.bssid);
    if let Some(next_candidate) =
        rotate_to_next_candidate(candidates, Some(rotate_from), &mut state.ap_candidate_idx)
    {
        if next_candidate.hint.bssid != rotate_from {
            selection.selected = next_candidate;
            selection.forced_rotation = true;
            diag_reassoc!(
                "upload_http: reason=other; forcing candidate rotation idx={} channel_hint={} bssid_hint={} count={}",
                state.ap_candidate_idx,
                next_candidate.hint.channel,
                format_bssid(next_candidate.hint.bssid),
                candidates.len(),
            );
        }
    }
    selection
}

fn preserve_auth_reject_hint(
    state: &mut WifiTaskState,
    disconnect_reason: u8,
    selected_ap: TargetApCandidate,
) -> bool {
    if !is_auth_disconnect_reason(disconnect_reason) {
        return false;
    }
    let Some(hinted_bssid) = state.bssid_hint else {
        return false;
    };
    let Some(hinted_idx) = state
        .ap_candidates
        .iter()
        .position(|candidate| candidate.hint.bssid == hinted_bssid)
    else {
        return false;
    };

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
    true
}

fn configure_hinted_other_retry(
    state: &mut WifiTaskState,
    hinted_idx: usize,
    hinted_ap: TargetApCandidate,
) {
    state.ap_candidate_idx = hinted_idx;
    state.channel_hint = Some(hinted_ap.hint.channel);
    state.bssid_hint = Some(hinted_ap.hint.bssid);
    state.auth_method_idx = (state.auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
    reset_candidate_retry_state(state);
    telemetry::record_wifi_reassoc_auth_rotation(
        state.auth_method_idx,
        state.channel_hint,
        state.channel_probe_idx,
    );

    if state.auth_method_idx != 0 {
        diag_reassoc!(
            "upload_http: reason=other streak={} preserving hinted candidate idx={} retry auth={:?} channel_hint={} bssid_hint={}",
            state.other_disconnect_streak,
            state.ap_candidate_idx,
            WIFI_AUTH_METHODS[state.auth_method_idx],
            hinted_ap.hint.channel,
            format_bssid(hinted_ap.hint.bssid),
        );
        return;
    }

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
}

fn configure_unpinned_other_retry(state: &mut WifiTaskState, selected_ap: TargetApCandidate) {
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
    reset_candidate_retry_state(state);
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
}

fn configure_repeated_other_retry(state: &mut WifiTaskState, selection: CandidateSelection) {
    if let Some((hinted_idx, hinted_ap)) = selection.hinted {
        configure_hinted_other_retry(state, hinted_idx, hinted_ap);
    } else {
        configure_unpinned_other_retry(state, selection.selected);
    }
}

fn configure_selected_candidate_retry(
    state: &mut WifiTaskState,
    selection: CandidateSelection,
) -> bool {
    if !selection.forced_rotation
        && state.channel_hint == Some(selection.selected.hint.channel)
        && state.bssid_hint == Some(selection.selected.hint.bssid)
    {
        return false;
    }

    state.channel_hint = Some(selection.selected.hint.channel);
    state.bssid_hint = Some(selection.selected.hint.bssid);
    state.auth_method_idx = 0;
    reset_candidate_retry_state(state);
    telemetry::record_wifi_reassoc_hint_retry(
        selection.selected.hint.channel,
        state.auth_method_idx,
        state.channel_probe_idx,
    );
    diag_reassoc!(
        "upload_http: retrying with candidate idx={} channel_hint={} bssid_hint={} count={}",
        state.ap_candidate_idx,
        selection.selected.hint.channel,
        format_bssid(selection.selected.hint.bssid),
        state.ap_candidates.len(),
    );
    true
}

async fn wait_for_candidate_retry() {
    Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
}

pub(super) async fn handle_observed_ap_paths(
    state: &mut WifiTaskState,
    input: ObservedApRecoveryInput,
) -> bool {
    let Some(observed_ap) = input.ap else {
        return false;
    };

    reset_observed_discovery_state(state);
    let selection = select_candidate(
        state,
        input.disconnect_reason,
        input.candidates.as_slice(),
        observed_ap,
    );
    state.ap_candidate_idx = input
        .candidates
        .iter()
        .position(|candidate| candidate.hint.bssid == selection.selected.hint.bssid)
        .unwrap_or(0);
    state.ap_candidates = input.candidates;

    if preserve_auth_reject_hint(state, input.disconnect_reason, selection.selected) {
        return false;
    }

    if input.disconnect_reason == WIFI_REASON_OTHER && state.other_disconnect_streak >= 2 {
        configure_repeated_other_retry(state, selection);
        wait_for_candidate_retry().await;
        return true;
    }

    if configure_selected_candidate_retry(state, selection) {
        wait_for_candidate_retry().await;
        return true;
    }

    diag_reassoc!(
        "upload_http: keeping discovered channel_hint={} bssid_hint={} for next auth attempt (candidate_count={})",
        observed_ap.hint.channel,
        format_bssid(observed_ap.hint.bssid),
        state.ap_candidates.len(),
    );
    false
}
