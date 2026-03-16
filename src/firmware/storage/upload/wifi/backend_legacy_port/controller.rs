extern crate alloc;

use super::{
    legacy_timer_compat_init_tasks_enabled, AccessPointInfo, ModeConfig, PowerSaveMode, Protocol,
    ScanConfig, WifiController, WifiError, WifiMode,
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

fn log_post_start_runtime_state(stage: &str) {
    if !legacy_timer_compat_enabled() {
        return;
    }

    let os_diag = esp_radio::diagnostic_wifi_os_diag_snapshot();
    let adapter_diag = esp_radio::diagnostic_wifi_adapter_primitive_diag();
    let scan_done = esp_radio::diagnostic_wifi_scan_done_eventpost_diag();
    let legacy_builtin = esp_radio::diagnostic_legacy_builtin_scheduler_diag();
    let legacy_preempt = esp_radio::diagnostic_legacy_preempt_builtin_diag();

    println!(
        "upload_http: legacy_port post_start after={} wifi_mac_isr_count={} queue_send={} queue_send_isr={} queue_recv={} event_post={} thread_sem_get={} task_get_current_task_count={} scan_done_count={} scan_done_ap_num={} legacy_builtin_initialized={} legacy_builtin_switch_count={} legacy_preempt_initialized={} legacy_preempt_current_task=0x{:x} legacy_preempt_thread_sem=0x{:x}",
        stage,
        esp_radio::diagnostic_wifi_mac_isr_count(),
        os_diag.queue_send,
        os_diag.queue_send_isr,
        os_diag.queue_recv,
        os_diag.event_post,
        adapter_diag.thread_sem_get_count,
        adapter_diag.task_get_current_task_count,
        scan_done.count,
        scan_done.ap_num,
        legacy_builtin.initialized as u8,
        legacy_builtin.switch_count,
        legacy_preempt.initialized as u8,
        legacy_preempt.current_task,
        legacy_preempt.current_task_thread_semaphore,
    );
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
    log_post_start_runtime_state("after_start");
    if legacy_timer_compat_enabled() && !legacy_timer_compat_init_tasks_enabled() {
        let status = esp_radio::backend_legacy_port_init_tasks();
        println!(
            "upload_http: legacy_port late_init_tasks result=ok timer_task_precreated={} yielded_once={}",
            status.timer_task_precreated,
            status.yielded_once,
        );
        log_post_start_runtime_state("after_late_init_tasks");
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
