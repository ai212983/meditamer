extern crate alloc;

use super::{
    legacy_timer_compat_init_tasks_enabled, AccessPointInfo, ModeConfig, PowerSaveMode, Protocol,
    ScanConfig, WifiController, WifiError, WifiMode,
};
use enumset::EnumSet;
use esp_println::println;
use esp_wifi_sys::include::{
    esp_wifi_clear_ap_list, esp_wifi_scan_get_ap_num, esp_wifi_scan_get_ap_records,
    esp_wifi_scan_start, wifi_active_scan_time_t, wifi_ap_record_t, wifi_scan_channel_bitmap_t,
    wifi_scan_config_t, wifi_scan_time_t, wifi_scan_type_t_WIFI_SCAN_TYPE_ACTIVE, ESP_OK,
};

fn legacy_port_force_direct_explicit_scan_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_FORCE_DIRECT_EXPLICIT_SCAN_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_FORCE_DIRECT_EXPLICIT_SCAN_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

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

fn direct_explicit_scan() -> Result<alloc::vec::Vec<AccessPointInfo>, WifiError> {
    let scan_config = wifi_scan_config_t {
        ssid: core::ptr::null_mut(),
        bssid: core::ptr::null_mut(),
        channel: 0,
        show_hidden: true,
        scan_type: wifi_scan_type_t_WIFI_SCAN_TYPE_ACTIVE,
        scan_time: wifi_scan_time_t {
            active: wifi_active_scan_time_t { min: 10, max: 20 },
            passive: 0,
        },
        home_chan_dwell_time: 0,
        channel_bitmap: wifi_scan_channel_bitmap_t {
            ghz_2_channels: 0,
            ghz_5_channels: 0,
        },
        coex_background_scan: false,
    };

    let scan_rc = unsafe { esp_wifi_scan_start(&scan_config, true) };
    if scan_rc != ESP_OK as i32 {
        return Err(WifiError::InternalError(
            esp_radio::wifi::InternalWifiError::Timeout,
        ));
    }

    let mut ap_num = 0u16;
    let ap_num_rc = unsafe { esp_wifi_scan_get_ap_num(&mut ap_num) };
    if ap_num_rc != ESP_OK as i32 {
        let _ = unsafe { esp_wifi_clear_ap_list() };
        return Err(WifiError::InternalError(
            esp_radio::wifi::InternalWifiError::Timeout,
        ));
    }

    if ap_num == 0 {
        return Ok(alloc::vec::Vec::new());
    }

    let mut returned = ap_num;
    let mut records =
        alloc::vec![unsafe { core::mem::zeroed::<wifi_ap_record_t>() }; ap_num as usize];
    let records_rc = unsafe { esp_wifi_scan_get_ap_records(&mut returned, records.as_mut_ptr()) };
    let _ = unsafe { esp_wifi_clear_ap_list() };
    if records_rc != ESP_OK as i32 {
        return Err(WifiError::InternalError(
            esp_radio::wifi::InternalWifiError::Timeout,
        ));
    }

    Ok(records
        .iter()
        .take(returned as usize)
        .map(esp_radio::wifi::access_point_info_from_raw_record)
        .collect())
}

pub(crate) async fn scan_with_config(
    controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<alloc::vec::Vec<AccessPointInfo>, WifiError> {
    if legacy_port_force_direct_explicit_scan_enabled() {
        return direct_explicit_scan();
    }
    esp_radio::wifi::WifiController::scan_with_config(controller, config)
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
    esp_radio::wifi::WifiController::start(controller)?;
    if legacy_timer_compat_enabled() && !legacy_timer_compat_init_tasks_enabled() {
        esp_rtos::precreate_esp_radio_timer_task();
        esp_rtos::yield_for_esp_radio_diag();
        println!("upload_http: legacy_port late_precreate_timer_task result=ok");
    }
    Ok(())
}

pub(crate) async fn stop(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::stop(controller)
}

pub(crate) async fn connect(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::connect(controller)
}

pub(crate) async fn disconnect(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::disconnect(controller)
}
