use super::*;

pub(super) async fn handle_connected_progress(
    controller: &mut WifiController<'_>,
    stack: &Stack<'_>,
    state: &mut WifiTaskState,
    dhcp_lease_observed: &mut bool,
    dhcp_wait_started_at: Instant,
    listener_wait_started_at: &mut Option<Instant>,
) -> bool {
    if !*dhcp_lease_observed {
        *dhcp_lease_observed = has_ipv4_lease(stack);
        if *dhcp_lease_observed {
            reset_listener_timeout_guard(state);
            let lease_ipv4 = stack_ipv4_lease(stack).unwrap_or([0, 0, 0, 0]);
            observability::set_wifi_ipv4(Some(lease_ipv4));
            let observability_ipv4 = observability::snapshot().wifi_ipv4.unwrap_or([0, 0, 0, 0]);
            diag_reassoc!(
                "upload_http: dhcp_ready lease_ipv4={}.{}.{}.{} observability_ipv4={}.{}.{}.{} listener_enabled={}",
                lease_ipv4[0],
                lease_ipv4[1],
                lease_ipv4[2],
                lease_ipv4[3],
                observability_ipv4[0],
                observability_ipv4[1],
                observability_ipv4[2],
                observability_ipv4[3],
                service_mode::upload_http_listener_enabled(),
            );
            *listener_wait_started_at = Some(Instant::now());
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
            reset_listener_timeout_guard(state);
            *listener_wait_started_at = None;
            let dhcp_timeout_ms = effective_dhcp_timeout_ms(
                state.runtime_policy,
                state.bssid_hint.is_some(),
                state.dhcp_same_candidate_timeout_streak,
            );
            if dhcp_wait_started_at.elapsed().as_millis() >= dhcp_timeout_ms as u64 {
                return recover_dhcp_no_ipv4_stall(
                    controller,
                    state,
                    dhcp_timeout_ms,
                    "dhcp_stall",
                    "dhcp_lease_reacquire",
                )
                .await;
            }
        }
    }

    if *dhcp_lease_observed {
        let listener_enabled = service_mode::upload_http_listener_enabled();
        let snapshot = observability::snapshot();
        let lease_ipv4 = stack_ipv4_lease(stack);
        observability::set_wifi_ipv4(lease_ipv4);
        if !listener_enabled {
            reset_listener_timeout_guard(state);
            observability::set_upload_http_listener(false, None);
            *listener_wait_started_at = None;
        }
        if !listener_enabled && lease_ipv4.is_some() {
            reset_listener_timeout_guard(state);
            *listener_wait_started_at = None;
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
            reset_listener_timeout_guard(state);
            *listener_wait_started_at = None;
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
        } else if listener_enabled && lease_ipv4.is_none() {
            reset_listener_timeout_guard(state);
            *listener_wait_started_at = None;
            let lease_loss_elapsed_ms = dhcp_wait_started_at.elapsed().as_millis() as u32;
            if lease_loss_elapsed_ms >= WIFI_LISTENER_LEASE_LOSS_GRACE_MS as u32 {
                return recover_dhcp_no_ipv4_stall(
                    controller,
                    state,
                    lease_loss_elapsed_ms,
                    "listener_ipv4_lost",
                    "listener_ipv4_lost",
                )
                .await;
            }
        } else if listener_enabled {
            let listener_wait_elapsed_ms = listener_wait_started_at
                .get_or_insert_with(Instant::now)
                .elapsed()
                .as_millis() as u32;
            if listener_wait_elapsed_ms < state.runtime_policy.listener_timeout_ms {
                return false;
            }
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
            let current_internal_free =
                psram::allocator_memory_snapshot().free_internal_bytes as u32;
            recover_listener_timeout(controller, state, current_internal_free).await;
            *listener_wait_started_at = None;
            return true;
        }
    }

    false
}
