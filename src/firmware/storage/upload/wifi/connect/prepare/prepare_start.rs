use super::*;

pub(super) async fn prepare_budget_and_start(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    active: WifiCredentials,
) -> bool {
    if state.net_attempt >= policy_total_attempt_budget(state.runtime_policy) {
        state.terminal_fail_latched = true;
        state.ladder_step = RecoveryLadderStep::TerminalFail;
        if matches!(state.failure_class, NetFailureClass::None) {
            state.failure_class = NetFailureClass::Unknown;
        }
        if state.failure_code == 0 {
            state.failure_code = WIFI_REASON_OTHER;
        }
        transition_state(
            &mut state.net_state,
            NetState::Failed,
            "attempt_budget_exhausted",
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
        Timer::after(Duration::from_millis(
            state.runtime_policy.cooldown_ms.max(250) as u64,
        ))
        .await;
        return true;
    }

    if !state.config_applied {
        let auth_method = WIFI_AUTH_METHODS[state.auth_method_idx];
        let mode = match mode_config_from_credentials(
            active,
            auth_method,
            state.channel_hint,
            state.bssid_hint,
        ) {
            Some(mode) => mode,
            None => {
                diag_wifi!("upload_http: wifi credentials invalid utf8 or length");
                state.credentials = None;
                return true;
            }
        };

        if let Err(err) = controller.set_config(&mode) {
            diag_wifi!("upload_http: wifi station config err={:?}", err);
            if matches!(controller.is_started(), Ok(true)) {
                let _ = controller.stop_async().await;
            }
            state.config_applied = false;
            Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
            return true;
        }
        diag_reassoc!(
            "upload_http: applying station config auth={:?} channel_hint={:?} bssid_hint={}",
            auth_method,
            state.channel_hint,
            format_bssid_opt(state.bssid_hint),
        );
        telemetry::record_wifi_reassoc_config_applied(
            state.auth_method_idx,
            state.channel_hint,
            state.channel_probe_idx,
        );
        state.config_applied = true;
    }

    match controller.is_started() {
        Ok(true) => {}
        Ok(false) => {
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
            if let Err(err) = controller.start_async().await {
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
                if state.hard_recover_watchdog_started_at.is_none() {
                    state.hard_recover_watchdog_started_at = Some(Instant::now());
                }
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
                    diag_reassoc!(
                        "upload_http: wifi start NoMem; forcing full wifi reset and hint clear"
                    );
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
                return true;
            }
            log_radio_mem_diag("start_ok");
            if let Err(err) = controller.set_power_saving(PowerSaveMode::None) {
                diag_wifi!("upload_http: wifi set power save none err={:?}", err);
            }
            telemetry::record_wifi_reassoc_start_ok();
            Timer::after(Duration::from_millis(WIFI_POST_START_SETTLE_MS)).await;
        }
        Err(err) => {
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
            if state.hard_recover_watchdog_started_at.is_none() {
                state.hard_recover_watchdog_started_at = Some(Instant::now());
            }
            let _ = controller.disconnect_async().await;
            let _ = controller.stop_async().await;
            state.config_applied = false;
            Timer::after(Duration::from_millis(
                state.runtime_policy.driver_restart_backoff_ms as u64,
            ))
            .await;
            return true;
        }
    }
    if state.escalated_auth_sweep_attempts_left > 0 {
        state.channel_hint = None;
        state.bssid_hint = None;
        state.ap_candidates.clear();
        state.ap_candidate_idx = 0;
        state.channel_probe_idx = 0;
    }

    false
}
