use super::*;

#[path = "success/success_progress.rs"]
mod success_progress;

use success_progress::handle_connected_progress;

pub(super) async fn handle_connect_success(
    controller: &mut WifiController<'static>,
    stack: &Stack<'static>,
    state: &mut WifiTaskState,
) {
    let mut dhcp_lease_observed = has_ipv4_lease(&stack);
    let dhcp_wait_started_at = Instant::now();
    loop {
        if !service_mode::upload_enabled() {
            telemetry::set_wifi_link_connected(false);
            telemetry::set_upload_http_listener(false, None);
            let _ = controller.disconnect_async().await;
            state.dhcp_lease_reacquire_attempts = 0;
            state.other_disconnect_streak = 0;
            state.hard_recover_watchdog_started_at = None;
            state.escalated_auth_sweep_attempts_left = 0;
            diag_wifi!("upload_http: upload mode off while connected");
            break;
        }

        apply_pending_runtime_policy_updates(&mut state.runtime_policy);

        let mut reconnect_due_to_credentials = false;
        while let Ok(updated) = WIFI_CREDENTIALS_UPDATES.try_receive() {
            if state.credentials == Some(updated) {
                diag_wifi!("upload_http: wifi credentials unchanged while connected");
                continue;
            }
            state.credentials = Some(updated);
            state.config_applied = false;
            state.auth_method_idx = 0;
            state.channel_hint = None;
            state.bssid_hint = None;
            state.ap_candidates.clear();
            state.ap_candidate_idx = 0;
            state.dhcp_same_candidate_timeout_streak = 0;
            state.dhcp_lease_reacquire_attempts = 0;
            state.other_disconnect_streak = 0;
            state.hard_recover_watchdog_started_at = None;
            state.escalated_auth_sweep_attempts_left = 0;
            state.channel_probe_idx = 0;
            reconnect_due_to_credentials = true;
        }
        if reconnect_due_to_credentials {
            diag_wifi!("upload_http: wifi credentials changed, reconnecting");
            telemetry::record_wifi_reassoc_credentials_changed();
            disconnect_with_timeout(controller, "credentials_changed").await;
            state.dhcp_lease_reacquire_attempts = 0;
            state.other_disconnect_streak = 0;
            state.hard_recover_watchdog_started_at = None;
            state.escalated_auth_sweep_attempts_left = 0;
            telemetry::set_wifi_link_connected(false);
            telemetry::set_upload_http_listener(false, None);
            break;
        }

        if WIFI_DISCONNECTED_EVENT.swap(false, Ordering::Relaxed) {
            let disconnect_reason = WIFI_LAST_DISCONNECT_REASON.load(Ordering::Relaxed);
            if disconnect_reason == WIFI_REASON_OTHER {
                state.other_disconnect_streak = state.other_disconnect_streak.saturating_add(1);
            } else if disconnect_reason != 0 {
                state.other_disconnect_streak = 0;
            }
            telemetry::record_wifi_reassoc_disconnect_event(disconnect_reason);
            state.dhcp_lease_reacquire_attempts = 0;
            telemetry::set_wifi_link_connected(false);
            telemetry::set_upload_http_listener(false, None);
            disconnect_with_timeout(controller, "connected_watchdog").await;
            state.config_applied = false;
            if disconnect_reason == WIFI_REASON_OTHER
                && state.other_disconnect_streak >= WIFI_REASON_OTHER_HARD_RECOVER_STREAK
            {
                state.channel_hint = None;
                state.bssid_hint = None;
                state.ap_candidates.clear();
                state.ap_candidate_idx = 0;
                state.channel_probe_idx = 0;
                state.auth_method_idx = 0;
                state.dhcp_same_candidate_timeout_streak = 0;
                state.dhcp_lease_reacquire_attempts = 0;
                state.other_disconnect_streak = 0;
                state.hard_recover_watchdog_started_at = Some(Instant::now());
                diag_reassoc!(
                    "upload_http: reason=other streak reached {}; forcing hard wifi recovery (stop/start + full discovery reset)",
                    WIFI_REASON_OTHER_HARD_RECOVER_STREAK
                );
                Timer::after(Duration::from_millis(
                    state.runtime_policy.driver_restart_backoff_ms as u64,
                ))
                .await;
                break;
            }
            if disconnect_reason == WIFI_REASON_OTHER
                || disconnect_reason == WIFI_REASON_BEACON_TIMEOUT
            {
                state.channel_hint = None;
                state.bssid_hint = None;
                state.ap_candidates.clear();
                state.ap_candidate_idx = 0;
                state.channel_probe_idx = 0;
                state.auth_method_idx = (state.auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
                state.dhcp_same_candidate_timeout_streak = 0;
                diag_reassoc!(
                    "upload_http: disconnect reason={} -> forcing full reconnect sweep auth={:?}",
                    disconnect_reason,
                    WIFI_AUTH_METHODS[state.auth_method_idx]
                );
            }
            diag_wifi!("upload_http: wifi disconnected");
            Timer::after(Duration::from_millis(WIFI_POST_DISCONNECT_SETTLE_MS)).await;
            break;
        }

        if handle_connected_progress(
            controller,
            stack,
            state,
            &mut dhcp_lease_observed,
            dhcp_wait_started_at,
        )
        .await
        {
            break;
        }

        Timer::after(Duration::from_millis(WIFI_CONNECTED_WATCHDOG_MS)).await;
    }
}
