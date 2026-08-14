extern crate alloc;

#[cfg(feature = "wifi-backend-esp-radio")]
#[path = "backend_esp_radio.rs"]
mod backend_esp_radio;

pub(crate) type WifiError = backend_esp_radio::WifiError;
pub(crate) type WifiController<'a> = backend_esp_radio::WifiController<'a>;
pub(crate) type WifiDevice = backend_esp_radio::WifiDevice;

pub(crate) use backend_esp_radio::{
    backend_name, initialize_runtime_sta, wifi_active_scan_config, wifi_callback_stats,
    wifi_channel_active_scan_config, wifi_client_mode_config, wifi_connect_async,
    wifi_directed_active_scan_config, wifi_disconnect_async, wifi_error_is_no_mem,
    wifi_finalize_shutdown, wifi_is_connected, wifi_is_started, wifi_passive_scan_config,
    wifi_power_save_none, wifi_raw_broad_scan_config, wifi_rssi, wifi_rx_buffer_stats,
    wifi_scan_with_config_async, wifi_set_config, wifi_set_mode, wifi_set_power_saving,
    wifi_set_protocol, wifi_shutdown_source, wifi_sta_mode, wifi_standard_bgn_protocols,
    wifi_start_async, wifi_stop_async, AccessPointInfo, AuthMethod, ModeConfig, ScanConfig,
};
