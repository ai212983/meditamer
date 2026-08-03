use super::*;

#[path = "success/success_progress.rs"]
mod success_progress;
#[path = "success/success_recovery.rs"]
mod success_recovery;

use success_progress::handle_connected_progress;
use success_recovery::recover_dhcp_no_ipv4_stall;

// Escalate from repeated listener-timeout reconnect loops into full stop/start
// before attempt-budget exhaustion to reduce stale internal-driver state drift.
const WIFI_LISTENER_TIMEOUT_HARD_RECOVER_STREAK: u8 = 6;
const WIFI_LISTENER_TIMEOUT_INTERNAL_FREE_DROP_BYTES: u32 = 1_024;

pub(super) fn reset_listener_timeout_guard(state: &mut WifiTaskState) {
    state.listener_timeout_streak = 0;
    state.listener_timeout_streak_start_internal_free = 0;
}

fn note_listener_timeout_guard(
    state: &mut WifiTaskState,
    current_internal_free: u32,
) -> (u8, u32, bool) {
    if state.listener_timeout_streak == 0 {
        state.listener_timeout_streak_start_internal_free = current_internal_free;
    }
    state.listener_timeout_streak = state.listener_timeout_streak.saturating_add(1);
    let internal_free_drop = state
        .listener_timeout_streak_start_internal_free
        .saturating_sub(current_internal_free);
    let hard_recover = state.listener_timeout_streak >= WIFI_LISTENER_TIMEOUT_HARD_RECOVER_STREAK
        || internal_free_drop >= WIFI_LISTENER_TIMEOUT_INTERNAL_FREE_DROP_BYTES;
    (
        state.listener_timeout_streak,
        internal_free_drop,
        hard_recover,
    )
}

pub(super) async fn recover_listener_timeout(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    current_internal_free: u32,
) {
    let (streak, internal_free_drop, hard_recover) =
        note_listener_timeout_guard(state, current_internal_free);
    diag_reassoc!(
        "upload_http: listener_timeout guard streak={} drop={}B baseline_internal_free={} current_internal_free={} hard_recover={}",
        streak,
        internal_free_drop,
        state.listener_timeout_streak_start_internal_free,
        current_internal_free,
        hard_recover,
    );

    if hard_recover {
        disconnect_and_stop_with_timeout(controller, "listener_timeout_hard_recover").await;
        state.config_applied = false;
        state.channel_hint = None;
        state.bssid_hint = None;
        state.ap_candidates.clear();
        state.ap_candidate_idx = 0;
        state.channel_probe_idx = 0;
        state.auth_method_idx = 0;
        state.dhcp_same_candidate_timeout_streak = 0;
        state.dhcp_lease_reacquire_attempts = 0;
        state.start_hard_recover_watchdog("listener_timeout_hard_recover");
        Timer::after(Duration::from_millis(
            state.runtime_policy.driver_restart_backoff_ms as u64,
        ))
        .await;
        reset_listener_timeout_guard(state);
    } else {
        disconnect_with_timeout(controller, "listener_timeout").await;
    }

    telemetry::set_wifi_link_connected(false);
    telemetry::set_upload_http_listener(false, None);
}

pub(super) async fn handle_connect_success(
    controller: &mut WifiController<'static>,
    stack: &Stack<'static>,
    state: &mut WifiTaskState,
) {
    let mut dhcp_lease_observed = has_ipv4_lease(stack);
    let dhcp_wait_started_at = Instant::now();
    let mut listener_wait_started_at = None;
    loop {
        if !service_mode::upload_enabled() {
            telemetry::set_wifi_link_connected(false);
            telemetry::set_upload_http_listener(false, None);
            let _ = wifi_disconnect_async(controller).await;
            state.dhcp_lease_reacquire_attempts = 0;
            reset_listener_timeout_guard(state);
            state.other_disconnect_streak = 0;
            state.clear_hard_recover_watchdog("upload_off_while_connected");
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
            reset_listener_timeout_guard(state);
            state.other_disconnect_streak = 0;
            state.clear_hard_recover_watchdog("credentials_changed_while_connected");
            state.escalated_auth_sweep_attempts_left = 0;
            state.channel_probe_idx = 0;
            reconnect_due_to_credentials = true;
        }
        if reconnect_due_to_credentials {
            diag_wifi!("upload_http: wifi credentials changed, reconnecting");
            telemetry::record_wifi_reassoc_credentials_changed();
            disconnect_with_timeout(controller, "credentials_changed").await;
            state.dhcp_lease_reacquire_attempts = 0;
            reset_listener_timeout_guard(state);
            state.other_disconnect_streak = 0;
            state.clear_hard_recover_watchdog("credentials_changed_reconnect");
            state.escalated_auth_sweep_attempts_left = 0;
            telemetry::set_wifi_link_connected(false);
            telemetry::set_upload_http_listener(false, None);
            break;
        }

        if !wifi_is_connected(controller) {
            WIFI_LAST_DISCONNECT_REASON
                .compare_exchange(0, WIFI_REASON_OTHER, Ordering::Relaxed, Ordering::Relaxed)
                .ok();
            WIFI_DISCONNECTED_EVENT.store(true, Ordering::Relaxed);
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
            reset_listener_timeout_guard(state);
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
                state.start_hard_recover_watchdog("connected_reason_other_hard_recover");
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
            &mut listener_wait_started_at,
        )
        .await
        {
            break;
        }

        if let Ok(rssi_dbm) = wifi_rssi(controller) {
            telemetry::record_wifi_link_rssi(rssi_dbm);
        }

        Timer::after(Duration::from_millis(WIFI_CONNECTED_WATCHDOG_MS)).await;
    }
}
