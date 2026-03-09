use super::*;

const WIFI_RESET_MAC_AFTER_STOP: bool =
    parse_nonzero_flag(match option_env!("MEDITAMER_WIFI_RESET_MAC_AFTER_STOP") {
        Some(value) => Some(value),
        None => option_env!("WIFI_RESET_MAC_AFTER_STOP"),
    });
const WIFI_MODE_NULL_STA_RESET_AFTER_STOP: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_MODE_NULL_STA_RESET_AFTER_STOP") {
        Some(value) => Some(value),
        None => option_env!("WIFI_MODE_NULL_STA_RESET_AFTER_STOP"),
    },
);
const WIFI_SOFTWARE_RESET_ON_ZERO_DISCOVERY_TERMINAL: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_SOFTWARE_RESET_ON_ZERO_DISCOVERY_TERMINAL") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SOFTWARE_RESET_ON_ZERO_DISCOVERY_TERMINAL"),
    },
);
const WIFI_SOFTWARE_RESET_ON_ZERO_DISCOVERY_HARD_GUARD: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_SOFTWARE_RESET_ON_ZERO_DISCOVERY_HARD_GUARD") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SOFTWARE_RESET_ON_ZERO_DISCOVERY_HARD_GUARD"),
    },
);

pub(super) async fn disconnect_with_timeout(
    controller: &mut WifiController<'static>,
    context: &str,
) {
    log_radio_mem_diag_with_trigger("recover_disconnect_before", context);
    match with_timeout(
        Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
        wifi_disconnect_async(controller),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            diag_reassoc!("upload_http: {} disconnect err={:?}", context, err);
        }
        Err(_) => {
            diag_reassoc!(
                "upload_http: {} disconnect timeout={}ms",
                context,
                WIFI_DRIVER_CONTROL_TIMEOUT_MS
            );
        }
    }
    log_radio_mem_diag_with_trigger("recover_disconnect_after", context);
}

pub(super) async fn disconnect_and_stop_with_timeout(
    controller: &mut WifiController<'static>,
    context: &str,
) {
    disconnect_with_timeout(controller, context).await;
    let mut stop_attempt = 0u8;
    let mut stopped = false;
    loop {
        log_radio_mem_diag_with_trigger("recover_stop_before", context);
        match with_timeout(
            Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
            wifi_stop_async(controller),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                diag_reassoc!(
                    "upload_http: {} stop err={:?} attempt={}",
                    context,
                    err,
                    stop_attempt + 1
                );
            }
            Err(_) => {
                diag_reassoc!(
                    "upload_http: {} stop timeout={}ms attempt={}",
                    context,
                    WIFI_DRIVER_CONTROL_TIMEOUT_MS,
                    stop_attempt + 1
                );
            }
        }
        log_radio_mem_diag_with_trigger("recover_stop_after", context);
        match wifi_is_started(controller) {
            Ok(false) => {
                stopped = true;
                break;
            }
            Ok(true) => {
                if stop_attempt >= WIFI_DRIVER_STOP_RETRIES {
                    diag_reassoc!(
                        "upload_http: {} stop retries exhausted; controller still started",
                        context
                    );
                    break;
                }
            }
            Err(err) => {
                diag_reassoc!(
                    "upload_http: {} is_started check err={:?} after stop",
                    context,
                    err
                );
                break;
            }
        }
        stop_attempt = stop_attempt.saturating_add(1);
        Timer::after(Duration::from_millis(WIFI_DRIVER_STOP_RETRY_BACKOFF_MS)).await;
    }
    if WIFI_RESET_MAC_AFTER_STOP && stopped {
        // Deeper radio reset than stop/start alone; avoids unsupported esp_wifi_restore path.
        unsafe { esp_hal::peripherals::WIFI::steal() }.reset_wifi_mac();
        diag_reassoc!("upload_http: {} reset_wifi_mac_after_stop applied", context);
        Timer::after(Duration::from_millis(WIFI_SHORT_SETTLE_MS)).await;
    }
    if WIFI_MODE_NULL_STA_RESET_AFTER_STOP && stopped {
        let set_null_rc = unsafe {
            esp_wifi_sys::include::esp_wifi_set_mode(
                esp_wifi_sys::include::wifi_mode_t_WIFI_MODE_NULL,
            )
        };
        let set_sta_rc = unsafe {
            esp_wifi_sys::include::esp_wifi_set_mode(
                esp_wifi_sys::include::wifi_mode_t_WIFI_MODE_STA,
            )
        };
        diag_reassoc!(
            "upload_http: {} mode_null_sta_reset_after_stop set_null_rc={} set_sta_rc={}",
            context,
            set_null_rc,
            set_sta_rc,
        );
        Timer::after(Duration::from_millis(WIFI_SHORT_SETTLE_MS)).await;
    }
}

pub(super) async fn disconnect_and_force_deep_reinit_with_timeout(
    controller: &mut WifiController<'static>,
    context: &str,
) {
    disconnect_and_stop_with_timeout(controller, context).await;
    match wifi_is_started(controller) {
        Ok(false) => {}
        Ok(true) => {
            diag_reassoc!(
                "upload_http: {} deep_reinit skipped controller_still_started=true",
                context
            );
            return;
        }
        Err(err) => {
            diag_reassoc!(
                "upload_http: {} deep_reinit is_started err={:?}",
                context,
                err
            );
            return;
        }
    }

    unsafe { esp_hal::peripherals::WIFI::steal() }.reset_wifi_mac();
    let set_null_rc = unsafe {
        esp_wifi_sys::include::esp_wifi_set_mode(esp_wifi_sys::include::wifi_mode_t_WIFI_MODE_NULL)
    };
    let set_sta_rc = unsafe {
        esp_wifi_sys::include::esp_wifi_set_mode(esp_wifi_sys::include::wifi_mode_t_WIFI_MODE_STA)
    };
    diag_reassoc!(
        "upload_http: {} deep_reinit applied reset_wifi_mac=true set_null_rc={} set_sta_rc={}",
        context,
        set_null_rc,
        set_sta_rc,
    );
    Timer::after(Duration::from_millis(WIFI_SHORT_SETTLE_MS)).await;
}

pub(super) async fn maybe_software_reset_on_zero_discovery_terminal(
    context: &str,
    sweep_streak: u8,
    hard_guard_restarts: u8,
) {
    if !WIFI_SOFTWARE_RESET_ON_ZERO_DISCOVERY_TERMINAL {
        return;
    }
    diag_reassoc!(
        "upload_http: {} zero_discovery_terminal software_reset=true sweep_streak={} hard_guard_restarts={}",
        context,
        sweep_streak,
        hard_guard_restarts,
    );
    // Give UART log transport a short window before hard reset.
    Timer::after(Duration::from_millis(250)).await;
    esp_hal::system::software_reset();
}

pub(super) async fn maybe_software_reset_on_zero_discovery_hard_guard(
    context: &str,
    hard_guard_trip: bool,
    sweep_streak: u8,
    hard_guard_restarts: u8,
) {
    if !WIFI_SOFTWARE_RESET_ON_ZERO_DISCOVERY_HARD_GUARD || !hard_guard_trip {
        return;
    }
    diag_reassoc!(
        "upload_http: {} zero_discovery_hard_guard software_reset=true sweep_streak={} hard_guard_restarts={}",
        context,
        sweep_streak,
        hard_guard_restarts,
    );
    Timer::after(Duration::from_millis(250)).await;
    esp_hal::system::software_reset();
}
