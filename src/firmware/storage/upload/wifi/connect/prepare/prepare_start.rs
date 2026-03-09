use super::*;

fn should_use_c_like_discovery_start(state: &WifiTaskState) -> bool {
    WIFI_C_LIKE_DISCOVERY_START
        && state.channel_hint.is_none()
        && state.bssid_hint.is_none()
        && state.ap_candidates.is_empty()
        && state.escalated_auth_sweep_attempts_left == 0
}

fn apply_station_config(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    active: WifiCredentials,
) -> Result<(), &'static str> {
    let auth_method = WIFI_AUTH_METHODS[state.auth_method_idx];
    let mode =
        mode_config_from_credentials(active, auth_method, state.channel_hint, state.bssid_hint)
            .ok_or("invalid_credentials")?;

    wifi_set_config(controller, &mode).map_err(|err| {
        diag_wifi!("upload_http: wifi station config err={:?}", err);
        "set_config_err"
    })?;
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
    Ok(())
}

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

    let c_like_discovery_start = should_use_c_like_discovery_start(state);

    if !state.config_applied && !c_like_discovery_start {
        match apply_station_config(controller, state, active) {
            Ok(()) => {}
            Err("invalid_credentials") => {
                diag_wifi!("upload_http: wifi credentials invalid utf8 or length");
                state.credentials = None;
                return true;
            }
            Err(_) => {
                if matches!(wifi_is_started(controller), Ok(true)) {
                    let _ = wifi_stop_async(controller).await;
                }
                state.config_applied = false;
                Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                return true;
            }
        }
    }

    match wifi_is_started(controller) {
        Ok(true) => {}
        Ok(false) => {
            if c_like_discovery_start {
                diag_reassoc!(
                    "upload_http: c_like_discovery_start enabled; starting bare STA before first scan"
                );
                if let Err(err) = wifi_set_mode(controller, wifi_sta_mode()) {
                    diag_wifi!("upload_http: bare sta mode set err={:?}", err);
                    Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                    return true;
                }
            }
            if WIFI_FORCE_STOP_BEFORE_START {
                diag_reassoc!(
                    "upload_http: force_stop_before_start enabled; issuing pre-start stop/disconnect"
                );
                disconnect_and_stop_with_timeout(controller, "force_stop_before_start").await;
                Timer::after(Duration::from_millis(WIFI_SHORT_SETTLE_MS)).await;
            }
            maybe_log_pre_start_driver_state(
                state.auth_method_idx,
                state.channel_hint,
                state.bssid_hint,
            );
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
            state.start_ok_at = Some(Instant::now());
            if c_like_discovery_start {
                diag_reassoc!(
                    "upload_http: c_like_discovery_start skipping post-start power-save/protocol mutation before scan"
                );
            } else {
                if let Err(err) = wifi_set_power_saving(controller, wifi_power_save_none()) {
                    diag_wifi!("upload_http: wifi set power save none err={:?}", err);
                }
                maybe_reapply_sta_protocol_after_start(controller);
            }
            maybe_log_first_start_driver_state(
                state.auth_method_idx,
                state.channel_hint,
                state.bssid_hint,
            );
            if maybe_handle_post_start_promisc_diag(controller, state).await {
                return true;
            }
            if WIFI_START_READINESS_PROBE {
                for step_ms in WIFI_START_READINESS_PROBE_STEPS_MS {
                    if step_ms > 0 {
                        Timer::after(Duration::from_millis(step_ms)).await;
                    }
                    match wifi_is_started(controller) {
                        Ok(started) => {
                            diag_reassoc!(
                                "upload_http: start_readiness_probe step_ms={} started={} start_ok_age_ms={} start_attempt_age_ms={} net_state={:?} ladder_step={:?} watchdog_active={}",
                                step_ms,
                                started,
                                WifiTaskState::point_age_ms(state.start_ok_at),
                                WifiTaskState::point_age_ms(state.start_attempt_started_at),
                                state.net_state,
                                state.ladder_step,
                                state.hard_recover_watchdog_started_at.is_some(),
                            );
                        }
                        Err(err) => {
                            diag_reassoc!(
                                "upload_http: start_readiness_probe step_ms={} status_err={:?} start_ok_age_ms={} start_attempt_age_ms={} net_state={:?} ladder_step={:?} watchdog_active={}",
                                step_ms,
                                err,
                                WifiTaskState::point_age_ms(state.start_ok_at),
                                WifiTaskState::point_age_ms(state.start_attempt_started_at),
                                state.net_state,
                                state.ladder_step,
                                state.hard_recover_watchdog_started_at.is_some(),
                            );
                        }
                    }
                }
            }
            if WIFI_START_RAW_SCAN_DIAG {
                let started_at = Instant::now();
                log_radio_mem_diag("start_raw_scan_diag_before");
                match with_timeout(
                    Duration::from_millis(WIFI_START_RAW_SCAN_DIAG_TIMEOUT_MS),
                    wifi_scan_with_config_async(controller, driver::raw_broad_scan_config()),
                )
                .await
                {
                    Ok(Ok(results)) => {
                        log_radio_mem_diag("start_raw_scan_diag_ok");
                        let top_channel = results.first().map(|ap| ap.channel).unwrap_or(0);
                        let top_bssid = format_bssid_opt(results.first().map(|ap| ap.bssid));
                        diag_reassoc!(
                            "upload_http: start_raw_scan_diag outcome=ok elapsed_ms={} result_count={} top_channel={} top_bssid={}",
                            elapsed_ms_u32(started_at),
                            results.len(),
                            top_channel,
                            top_bssid,
                        );
                    }
                    Ok(Err(err)) => {
                        log_radio_mem_diag("start_raw_scan_diag_err");
                        diag_reassoc!(
                            "upload_http: start_raw_scan_diag outcome=err elapsed_ms={} err={:?}",
                            elapsed_ms_u32(started_at),
                            err,
                        );
                    }
                    Err(_) => {
                        log_radio_mem_diag("start_raw_scan_diag_timeout");
                        diag_reassoc!(
                            "upload_http: start_raw_scan_diag outcome=timeout elapsed_ms={} timeout_ms={}",
                            elapsed_ms_u32(started_at),
                            WIFI_START_RAW_SCAN_DIAG_TIMEOUT_MS,
                        );
                    }
                }
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
            state.start_hard_recover_watchdog("status_err");
            let _ = wifi_disconnect_async(controller).await;
            let _ = wifi_stop_async(controller).await;
            state.config_applied = false;
            Timer::after(Duration::from_millis(
                state.runtime_policy.driver_restart_backoff_ms as u64,
            ))
            .await;
            return true;
        }
    }

    if !state.config_applied {
        match apply_station_config(controller, state, active) {
            Ok(()) => {}
            Err("invalid_credentials") => {
                diag_wifi!("upload_http: wifi credentials invalid utf8 or length");
                state.credentials = None;
                return true;
            }
            Err(_) => {
                if matches!(wifi_is_started(controller), Ok(true)) {
                    let _ = wifi_stop_async(controller).await;
                }
                state.config_applied = false;
                Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                return true;
            }
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
