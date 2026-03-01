use super::*;
pub(super) async fn perform_connect_attempt(
    controller: &mut WifiController<'static>,
    stack: &Stack<'static>,
    state: &mut WifiTaskState,
) {
    let connect_started_at = Instant::now();
    match with_timeout(
        Duration::from_millis(state.runtime_policy.connect_timeout_ms as u64),
        controller.connect_async(),
    )
    .await
    {
        Ok(Ok(())) => {
            telemetry::record_wifi_connect_success();
            telemetry::record_wifi_reassoc_connect_success(elapsed_ms_u32(connect_started_at));
            state.hard_recover_watchdog_started_at = None;
            state.discovery_sweep_exhausted_streak = 0;
            state.zero_discovery_hard_guard_restarts = 0;
            state.force_full_channel_probe_next_scan = false;
            state.escalated_auth_sweep_attempts_left = 0;
            state.failure_class = NetFailureClass::None;
            state.failure_code = 0;
            diag_wifi!("upload_http: wifi connected");
            transition_state(
                &mut state.net_state,
                NetState::DhcpWait,
                "connect_ok",
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
            handle_connect_success(controller, stack, state).await;
        }
        Ok(Err(err)) => {
            handle_connect_error(controller, state, err, connect_started_at).await;
        }
        Err(_) => {
            handle_connect_timeout(controller, state, connect_started_at).await;
        }
    }
}
