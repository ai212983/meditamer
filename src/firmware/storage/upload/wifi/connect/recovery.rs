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
    if WIFI_RESET_MAC_AFTER_STOP || WIFI_MODE_NULL_STA_RESET_AFTER_STOP {
        diag_reassoc!(
            "upload_http: {} raw stop/reset unavailable on esp-radio 1.0; disconnect/reconfigure selected",
            context,
        );
    }
}

pub(super) async fn disconnect_and_force_deep_reinit_with_timeout(
    controller: &mut WifiController<'static>,
    context: &str,
) {
    disconnect_and_stop_with_timeout(controller, context).await;
    diag_reassoc!(
        "upload_http: {} deep_reinit mapped to disconnect/reconfigure for esp-radio 1.0",
        context,
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
