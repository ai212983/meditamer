use super::*;
use crate::firmware::storage::upload::wifi::diag::publish_radio_quiesced;

pub(super) enum PreparePreconditions {
    Continue,
    Active(WifiCredentials),
}

pub(super) async fn prepare_preconditions(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
) -> PreparePreconditions {
    apply_pending_runtime_policy_updates(&mut state.runtime_policy);

    while let Ok(config) = NET_CONFIG_SET_UPDATES.try_receive() {
        state.runtime_policy = config.policy.sanitized();
        if let Some(updated) = config.credentials {
            if state.credentials != Some(updated) {
                state.credentials = Some(updated);
                telemetry::record_wifi_reassoc_credentials_changed();
            }
        }
        state.net_attempt = 0;
        state.terminal_fail_latched = false;
        publish_config(state.credentials, state.runtime_policy);
    }

    while let Ok(control) = NET_CONTROL_COMMANDS.try_receive() {
        if matches!(control, NetControlCommand::Recover) {
            state.config_applied = false;
            state.auth_method_idx = 0;
            state.channel_hint = None;
            state.bssid_hint = None;
            state.ap_candidates.clear();
            state.ap_candidate_idx = 0;
            state.channel_probe_idx = 0;
            state.dhcp_same_candidate_timeout_streak = 0;
            state.dhcp_lease_reacquire_attempts = 0;
            state.other_disconnect_streak = 0;
            state.discovery_sweep_exhausted_streak = 0;
            state.zero_discovery_hard_guard_restarts = 0;
            state.force_full_channel_probe_next_scan = false;
            state.start_hard_recover_watchdog("host_recover");
            state.escalated_auth_sweep_attempts_left = 0;
            state.terminal_fail_latched = false;
            state.net_attempt = 0;
            state.ladder_step = RecoveryLadderStep::DriverRestart;
            state.failure_class = NetFailureClass::None;
            state.failure_code = 0;
            transition_state(
                &mut state.net_state,
                NetState::Recovering,
                "host_recover",
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
        }
    }
    publish_config(state.credentials, state.runtime_policy);

    if !service_mode::upload_enabled() {
        if !state.paused {
            publish_radio_quiesced(false);
            disconnect_and_stop_with_timeout(controller, "upload_off_pause").await;
            telemetry::set_wifi_link_connected(false);
            telemetry::set_upload_http_listener(false, None);
            telemetry::record_wifi_reassoc_mode_pause();
            state.paused = true;
            publish_radio_quiesced(true);
            state.config_applied = false;
            state.auth_method_idx = 0;
            state.channel_hint = None;
            state.bssid_hint = None;
            state.ap_candidates.clear();
            state.ap_candidate_idx = 0;
            state.channel_probe_idx = 0;
            state.dhcp_lease_reacquire_attempts = 0;
            state.other_disconnect_streak = 0;
            state.discovery_sweep_exhausted_streak = 0;
            state.zero_discovery_hard_guard_restarts = 0;
            state.force_full_channel_probe_next_scan = false;
            state.clear_hard_recover_watchdog("upload_off_pause");
            state.escalated_auth_sweep_attempts_left = 0;
            state.terminal_fail_latched = false;
            state.net_attempt = 0;
            diag_wifi!("upload_http: upload mode off; wifi paused");
            transition_state(
                &mut state.net_state,
                NetState::Idle,
                "upload_off",
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
        }
        Timer::after(Duration::from_millis(WIFI_SHORT_SETTLE_MS)).await;
        return PreparePreconditions::Continue;
    }

    if state.paused {
        state.paused = false;
        publish_radio_quiesced(false);
        state.net_attempt = 0;
        state.terminal_fail_latched = false;
        telemetry::record_wifi_reassoc_mode_resume();
        diag_wifi!("upload_http: upload mode on; wifi resuming");
        transition_state(
            &mut state.net_state,
            NetState::Starting,
            "upload_on",
            state.started_at,
            state.ladder_step,
            state.net_attempt,
            (state.failure_class, state.failure_code),
        );
    }
    if state.terminal_fail_latched {
        Timer::after(Duration::from_millis(
            state.runtime_policy.cooldown_ms.max(250) as u64,
        ))
        .await;
        return PreparePreconditions::Continue;
    }
    if let Some(watchdog_started_at) = state.hard_recover_watchdog_started_at {
        let elapsed_ms = watchdog_started_at.elapsed().as_millis();
        let watchdog_timeout_ms = post_recover_watchdog_timeout_ms(state.runtime_policy);
        if elapsed_ms >= watchdog_timeout_ms {
            telemetry::record_wifi_reassoc_disconnect_event(
                WIFI_REASON_POST_HARD_RECOVER_CONNECT_STALL,
            );
            let last_scan_age_ms =
                WifiTaskState::point_age_ms(state.hard_recover_watchdog_last_scan_completed_at);
            let last_connect_begin_age_ms =
                WifiTaskState::point_age_ms(state.hard_recover_watchdog_last_connect_begin_at);
            println!(
                "upload_http: post-hard-recover-connect-stall elapsed_ms={} watchdog_timeout_ms={} connect_timeout_ms={} start_reason={} scan_rounds={} zero_scan_rounds={} connect_begins={} last_scan_age_ms={} last_connect_begin_age_ms={} forcing full restart",
                elapsed_ms,
                watchdog_timeout_ms,
                state.runtime_policy.connect_timeout_ms,
                state.hard_recover_watchdog_start_reason,
                state.hard_recover_watchdog_scan_rounds,
                state.hard_recover_watchdog_zero_scan_rounds,
                state.hard_recover_watchdog_connect_begins,
                last_scan_age_ms,
                last_connect_begin_age_ms,
            );
            disconnect_and_stop_with_timeout(controller, "post_recover_watchdog").await;
            telemetry::set_wifi_link_connected(false);
            telemetry::set_upload_http_listener(false, None);
            state.config_applied = false;
            state.auth_method_idx = 0;
            state.channel_hint = None;
            state.bssid_hint = None;
            state.ap_candidates.clear();
            state.ap_candidate_idx = 0;
            state.channel_probe_idx = 0;
            state.dhcp_same_candidate_timeout_streak = 0;
            state.dhcp_lease_reacquire_attempts = 0;
            state.other_disconnect_streak = 0;
            state.discovery_sweep_exhausted_streak = 0;
            state.zero_discovery_hard_guard_restarts = 0;
            state.force_full_channel_probe_next_scan = false;
            state.clear_hard_recover_watchdog("post_recover_watchdog_trip");
            state.start_hard_recover_watchdog("post_recover_watchdog_restart_cycle");
            state.escalated_auth_sweep_attempts_left = WIFI_ESCALATED_AUTH_SWEEP_ATTEMPTS;
            state.ladder_step = RecoveryLadderStep::DriverRestart;
            state.failure_class = NetFailureClass::PostRecoverStall;
            state.failure_code = WIFI_REASON_POST_HARD_RECOVER_CONNECT_STALL;
            transition_state(
                &mut state.net_state,
                NetState::Recovering,
                "post_recover_watchdog",
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
            println!(
                "upload_http: post-hard-recover-escalated-scan begin attempts={} watchdog_timeout_ms={} connect_timeout_ms={} start_reason={}",
                state.escalated_auth_sweep_attempts_left,
                watchdog_timeout_ms,
                state.runtime_policy.connect_timeout_ms,
                state.hard_recover_watchdog_start_reason,
            );
            Timer::after(Duration::from_millis(
                state.runtime_policy.driver_restart_backoff_ms as u64,
            ))
            .await;
            return PreparePreconditions::Continue;
        }
    }

    while let Ok(updated) = WIFI_CREDENTIALS_UPDATES.try_receive() {
        if state.credentials == Some(updated) {
            diag_wifi!("upload_http: wifi credentials unchanged; skipping reconfigure");
            continue;
        }
        state.credentials = Some(updated);
        state.config_applied = false;
        state.auth_method_idx = 0;
        state.channel_hint = None;
        state.bssid_hint = None;
        state.ap_candidates.clear();
        state.ap_candidate_idx = 0;
        state.channel_probe_idx = 0;
        state.dhcp_lease_reacquire_attempts = 0;
        state.other_disconnect_streak = 0;
        state.discovery_sweep_exhausted_streak = 0;
        state.zero_discovery_hard_guard_restarts = 0;
        state.force_full_channel_probe_next_scan = false;
        state.clear_hard_recover_watchdog("credentials_updated");
        state.escalated_auth_sweep_attempts_left = 0;
        state.net_attempt = 0;
        state.terminal_fail_latched = false;
        telemetry::record_wifi_reassoc_credentials_changed();
        diag_wifi!("upload_http: wifi credentials updated");
    }

    let active = match state.credentials {
        Some(value) => value,
        None => {
            if let Ok(first) = with_timeout(
                Duration::from_secs(WIFI_WAIT_CREDENTIALS_TIMEOUT_S),
                WIFI_CREDENTIALS_UPDATES.receive(),
            )
            .await
            {
                state.credentials = Some(first);
                state.config_applied = false;
                state.auth_method_idx = 0;
                state.channel_hint = None;
                state.bssid_hint = None;
                state.ap_candidates.clear();
                state.ap_candidate_idx = 0;
                state.channel_probe_idx = 0;
                state.dhcp_lease_reacquire_attempts = 0;
                state.other_disconnect_streak = 0;
                state.discovery_sweep_exhausted_streak = 0;
                state.zero_discovery_hard_guard_restarts = 0;
                state.force_full_channel_probe_next_scan = false;
                state.clear_hard_recover_watchdog("credentials_received");
                state.escalated_auth_sweep_attempts_left = 0;
                state.net_attempt = 0;
                state.terminal_fail_latched = false;
                telemetry::record_wifi_reassoc_credentials_received();
                publish_config(state.credentials, state.runtime_policy);
                diag_wifi!("upload_http: wifi credentials received");
            }
            return PreparePreconditions::Continue;
        }
    };

    PreparePreconditions::Active(active)
}
