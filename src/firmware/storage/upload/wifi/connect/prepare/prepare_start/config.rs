use super::*;

pub(super) fn should_use_c_like_discovery_start(state: &WifiTaskState) -> bool {
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

pub(super) async fn ensure_station_config_applied(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    active: WifiCredentials,
) -> bool {
    match apply_station_config(controller, state, active) {
        Ok(()) => false,
        Err("invalid_credentials") => {
            diag_wifi!("upload_http: wifi credentials invalid utf8 or length");
            state.credentials = None;
            true
        }
        Err(_) => {
            if matches!(wifi_is_started(controller), Ok(true)) {
                let _ = wifi_stop_async(controller).await;
            }
            state.config_applied = false;
            Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
            true
        }
    }
}
