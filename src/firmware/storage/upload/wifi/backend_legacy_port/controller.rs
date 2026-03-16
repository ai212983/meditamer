extern crate alloc;

use super::{
    legacy_timer_compat_init_tasks_enabled, log_runtime_state, AccessPointInfo, ModeConfig,
    PowerSaveMode, Protocol, ScanConfig, WifiController, WifiError, WifiMode,
};
use enumset::EnumSet;
use esp_println::println;

fn legacy_timer_compat_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TIMER_COMPAT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_TIMER_COMPAT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

pub(crate) async fn scan_with_config(
    controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<alloc::vec::Vec<AccessPointInfo>, WifiError> {
    esp_radio::wifi::backend_legacy_port_scan_with_config(controller, config)
}

pub(crate) fn set_config(
    controller: &mut WifiController<'_>,
    conf: &ModeConfig,
) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::set_config(controller, conf)
}

pub(crate) fn set_mode(
    controller: &mut WifiController<'_>,
    mode: WifiMode,
) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::set_mode(controller, mode)
}

pub(crate) fn is_started(controller: &WifiController<'_>) -> Result<bool, WifiError> {
    esp_radio::wifi::WifiController::is_started(controller)
}

pub(crate) fn set_power_saving(
    controller: &mut WifiController<'_>,
    ps: PowerSaveMode,
) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::set_power_saving(controller, ps)
}

pub(crate) fn set_protocol(
    controller: &mut WifiController<'_>,
    protocols: EnumSet<Protocol>,
) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::set_protocol(controller, protocols)
}

pub(crate) fn rssi(controller: &WifiController<'_>) -> Result<i32, WifiError> {
    esp_radio::wifi::WifiController::rssi(controller)
}

pub(crate) async fn start(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_radio::wifi::backend_legacy_port_start(controller)?;
    log_runtime_state("after_start");
    if legacy_timer_compat_enabled() && !legacy_timer_compat_init_tasks_enabled() {
        let status = esp_radio::backend_legacy_port_init_tasks();
        println!(
            "upload_http: legacy_port late_init_tasks result=ok timer_task_precreated={} yielded_once={}",
            status.timer_task_precreated,
            status.yielded_once,
        );
        log_runtime_state("after_late_init_tasks");
    }
    Ok(())
}

pub(crate) async fn stop(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_radio::wifi::backend_legacy_port_stop(controller)
}

pub(crate) async fn connect(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::connect(controller)
}

pub(crate) async fn disconnect(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::disconnect(controller)
}
