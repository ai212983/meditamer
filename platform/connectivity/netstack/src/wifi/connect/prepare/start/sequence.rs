use super::*;

pub(super) async fn run_start_driver(
    controller: &mut WifiController<'_>,
    state: &mut WifiTaskState,
) -> bool {
    transition_state(
        &mut state.net_state,
        NetState::Starting,
        "start_driver",
        state.started_at,
        state.ladder_step,
        state.net_attempt,
        (state.failure_class, state.failure_code),
    );
    log_radio_mem_diag("start_before");
    state.start_attempt_started_at = Some(Instant::now());
    if let Err(err) = wifi_start_async(controller).await {
        return handle_start_err(controller, state, err).await;
    }

    log_radio_mem_diag("start_ok");
    state.start_ok_at = Some(Instant::now());
    if let Err(err) = wifi_set_power_saving(controller, wifi_power_save_none()) {
        diag_wifi!("upload_http: wifi set power save none err={:?}", err);
    }
    telemetry::record_wifi_reassoc_start_ok();
    Timer::after(Duration::from_millis(WIFI_POST_START_SETTLE_MS)).await;
    false
}

async fn handle_start_err(
    controller: &mut WifiController<'_>,
    state: &mut WifiTaskState,
    err: WifiError,
) -> bool {
    diag_wifi!("upload_http: wifi start err={:?}", err);
    log_radio_mem_diag("start_err");
    telemetry::record_wifi_reassoc_start_err();
    state.config_applied = false;
    state.failure_class = NetFailureClass::Transport;
    state.failure_code = WIFI_REASON_OTHER;
    state.ladder_step = RecoveryLadderStep::DriverRestart;
    transition_state(
        &mut state.net_state,
        NetState::Recovering,
        "start_err",
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
    state.start_hard_recover_watchdog("start_err");
    if is_no_mem_wifi_error(&err) {
        disconnect_and_stop_with_timeout(controller, "start_nomem").await;
        state.channel_hint = None;
        state.bssid_hint = None;
        state.ap_candidates.clear();
        state.ap_candidate_idx = 0;
        state.auth_method_idx = 0;
        state.channel_probe_idx = 0;
        state.dhcp_same_candidate_timeout_streak = 0;
        state.dhcp_lease_reacquire_attempts = 0;
        state.other_disconnect_streak = 0;
        diag_reassoc!("upload_http: wifi start NoMem; forcing full wifi reset and hint clear");
        log_radio_mem_diag("start_nomem");
        state.ladder_step = RecoveryLadderStep::DriverRestart;
        state.failure_class = NetFailureClass::Transport;
        state.failure_code = WIFI_REASON_START_NOMEM;
        transition_state(
            &mut state.net_state,
            NetState::Recovering,
            "start_nomem",
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
        Timer::after(Duration::from_millis(WIFI_NOMEM_RECOVERY_BACKOFF_MS)).await;
        return true;
    }
    disconnect_and_stop_with_timeout(controller, "start_err").await;
    Timer::after(Duration::from_millis(
        state.runtime_policy.driver_restart_backoff_ms as u64,
    ))
    .await;
    true
}

pub(super) async fn handle_status_err(
    controller: &mut WifiController<'_>,
    state: &mut WifiTaskState,
    err: WifiError,
) -> bool {
    diag_wifi!("upload_http: wifi status err={:?}", err);
    state.failure_class = NetFailureClass::Transport;
    state.failure_code = WIFI_REASON_OTHER;
    state.ladder_step = RecoveryLadderStep::DriverRestart;
    transition_state(
        &mut state.net_state,
        NetState::Recovering,
        "status_err",
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
    state.start_hard_recover_watchdog("status_err");
    let _ = wifi_disconnect_async(controller).await;
    let _ = wifi_stop_async(controller).await;
    state.config_applied = false;
    Timer::after(Duration::from_millis(
        state.runtime_policy.driver_restart_backoff_ms as u64,
    ))
    .await;
    true
}
