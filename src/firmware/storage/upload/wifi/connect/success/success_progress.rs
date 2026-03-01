use super::*;

pub(super) async fn handle_connected_progress(
    controller: &mut WifiController<'static>,
    stack: &Stack<'static>,
    state: &mut WifiTaskState,
    dhcp_lease_observed: &mut bool,
    dhcp_wait_started_at: Instant,
) -> bool {
    if !*dhcp_lease_observed {
        *dhcp_lease_observed = has_ipv4_lease(&stack);
        if *dhcp_lease_observed {
            transition_state(
                &mut state.net_state,
                NetState::ListenerWait,
                "dhcp_ready",
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
            state.dhcp_same_candidate_timeout_streak = 0;
            state.dhcp_lease_reacquire_attempts = 0;
            state.other_disconnect_streak = 0;
        } else {
            let dhcp_timeout_ms = effective_dhcp_timeout_ms(
                state.runtime_policy,
                state.bssid_hint.is_some(),
                state.dhcp_same_candidate_timeout_streak,
            );
            if dhcp_wait_started_at.elapsed().as_millis() >= dhcp_timeout_ms as u64 {
                telemetry::record_wifi_reassoc_disconnect_event(WIFI_REASON_DHCP_NO_IPV4_STALL);
                telemetry::record_wifi_watchdog_disconnect();
                state.failure_class = NetFailureClass::DhcpNoIpv4;
                state.failure_code = WIFI_REASON_DHCP_NO_IPV4_STALL;
                state.ladder_step = RecoveryLadderStep::RetrySame;
                transition_state(
                    &mut state.net_state,
                    NetState::Recovering,
                    "dhcp_stall",
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
                if state.dhcp_lease_reacquire_attempts < WIFI_DHCP_LEASE_REACQUIRE_MAX_ATTEMPTS {
                    state.dhcp_lease_reacquire_attempts =
                        state.dhcp_lease_reacquire_attempts.saturating_add(1);
                    state.config_applied = false;
                    diag_reassoc!(
                            "upload_http: dhcp/no-ipv4 stall; lease reacquire attempt {}/{} auth={:?} channel_hint={:?} bssid_hint={}",
                            state.dhcp_lease_reacquire_attempts,
                            WIFI_DHCP_LEASE_REACQUIRE_MAX_ATTEMPTS,
                            WIFI_AUTH_METHODS[state.auth_method_idx],
                            state.channel_hint,
                            format_bssid_opt(state.bssid_hint),
                        );
                    disconnect_with_timeout(controller, "dhcp_lease_reacquire").await;
                    telemetry::set_wifi_link_connected(false);
                    telemetry::set_upload_http_listener(false, None);
                    Timer::after(Duration::from_millis(WIFI_DHCP_LEASE_REACQUIRE_BACKOFF_MS)).await;
                    return true;
                }

                state.dhcp_lease_reacquire_attempts = 0;
                let previous_bssid = state.bssid_hint;
                if previous_bssid.is_some() {
                    diag_wifi!(
                            "upload_http: dhcp/no-ipv4 stall on pinned bssid after {}ms; clearing bssid hint and reconnecting",
                            dhcp_timeout_ms
                        );
                } else {
                    diag_wifi!(
                            "upload_http: dhcp/no-ipv4 stall after {}ms; reconnecting and retrying scan/auth",
                            dhcp_timeout_ms
                        );
                    state.channel_probe_idx = 0;
                }
                if let Some(next_candidate) = rotate_to_next_candidate(
                    &state.ap_candidates,
                    previous_bssid,
                    &mut state.ap_candidate_idx,
                ) {
                    state.channel_hint = Some(next_candidate.hint.channel);
                    state.bssid_hint = Some(next_candidate.hint.bssid);
                    state.auth_method_idx = 0;
                    state.config_applied = false;
                    if previous_bssid == Some(next_candidate.hint.bssid) {
                        state.dhcp_same_candidate_timeout_streak =
                            state.dhcp_same_candidate_timeout_streak.saturating_add(1);
                    } else {
                        state.dhcp_same_candidate_timeout_streak = 0;
                    }
                    diag_reassoc!(
                            "upload_http: dhcp/no-ipv4 stall candidate rotate idx={} channel_hint={} bssid_hint={} same_streak={} candidates={}",
                            state.ap_candidate_idx,
                            next_candidate.hint.channel,
                            format_bssid(next_candidate.hint.bssid),
                            state.dhcp_same_candidate_timeout_streak,
                            state.ap_candidates.len(),
                        );
                } else {
                    state.dhcp_same_candidate_timeout_streak =
                        state.dhcp_same_candidate_timeout_streak.saturating_add(1);
                    state.channel_hint = None;
                    state.bssid_hint = None;
                    state.auth_method_idx = 0;
                    state.config_applied = false;
                    state.channel_probe_idx = 0;
                    diag_reassoc!(
                            "upload_http: dhcp/no-ipv4 stall no candidate available; forcing fresh discovery streak={}",
                            state.dhcp_same_candidate_timeout_streak,
                        );
                }
                let _ = controller.disconnect_async().await;
                if state.dhcp_same_candidate_timeout_streak
                    >= WIFI_DHCP_SAME_CANDIDATE_RESTART_STREAK
                {
                    diag_reassoc!(
                            "upload_http: dhcp/no-ipv4 stall streak={} reached; forcing wifi stop/start and full rescan",
                            state.dhcp_same_candidate_timeout_streak,
                        );
                    let _ = controller.stop_async().await;
                    state.ap_candidates.clear();
                    state.ap_candidate_idx = 0;
                    state.channel_hint = None;
                    state.bssid_hint = None;
                    state.auth_method_idx = 0;
                    state.config_applied = false;
                    state.channel_probe_idx = 0;
                    state.dhcp_same_candidate_timeout_streak = 0;
                }
                telemetry::set_wifi_link_connected(false);
                telemetry::set_upload_http_listener(false, None);
                return true;
            }
        }
    }

    if *dhcp_lease_observed {
        let listener_enabled = service_mode::upload_http_listener_enabled();
        let snapshot = telemetry::snapshot();
        let lease_ipv4 = stack_ipv4_lease(&stack);
        if !listener_enabled {
            telemetry::set_upload_http_listener(false, lease_ipv4);
        }
        if !listener_enabled && lease_ipv4.is_some() {
            transition_state(
                &mut state.net_state,
                NetState::Ready,
                "listener_bypass_ready",
                state.started_at,
                state.ladder_step,
                state.net_attempt,
                (state.failure_class, state.failure_code),
            );
            state.net_attempt = 0;
            state.ladder_step = RecoveryLadderStep::RetrySame;
            state.failure_class = NetFailureClass::None;
            state.failure_code = 0;
            publish_state(
                state.net_state,
                state.ladder_step,
                state.net_attempt,
                state.failure_class,
                state.failure_code,
                state.started_at.elapsed().as_millis() as u32,
            );
        } else if snapshot.upload_http_listening && snapshot.upload_http_ipv4.is_some() {
            transition_state(
                &mut state.net_state,
                NetState::Ready,
                "listener_ready",
                state.started_at,
                state.ladder_step,
                state.net_attempt,
                (state.failure_class, state.failure_code),
            );
            state.net_attempt = 0;
            state.ladder_step = RecoveryLadderStep::RetrySame;
            state.failure_class = NetFailureClass::None;
            state.failure_code = 0;
            publish_state(
                state.net_state,
                state.ladder_step,
                state.net_attempt,
                state.failure_class,
                state.failure_code,
                state.started_at.elapsed().as_millis() as u32,
            );
        } else if listener_enabled
            && dhcp_wait_started_at.elapsed().as_millis()
                >= state.runtime_policy.listener_timeout_ms as u64
        {
            state.failure_class = NetFailureClass::ListenerNotReady;
            state.failure_code = 1;
            state.ladder_step = RecoveryLadderStep::RetrySame;
            transition_state(
                &mut state.net_state,
                NetState::Recovering,
                "listener_timeout",
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
            disconnect_with_timeout(controller, "listener_timeout").await;
            telemetry::set_wifi_link_connected(false);
            telemetry::set_upload_http_listener(false, None);
            return true;
        }
    }

    false
}
