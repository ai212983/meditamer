extern crate alloc;

#[cfg(feature = "wifi-backend-esp-radio")]
#[path = "backend_esp_radio.rs"]
mod backend_esp_radio;
#[path = "backend_owner/mod.rs"]
mod backend_owner;

pub(crate) type RadioController = backend_esp_radio::RadioController;
pub(crate) type WifiError = backend_esp_radio::WifiError;
pub(crate) type WifiDriverConfig = backend_esp_radio::WifiDriverConfig;
pub(crate) type WifiController<'a> = backend_esp_radio::WifiController<'a>;
pub(crate) type WifiDevice<'a> = backend_esp_radio::WifiDevice<'a>;

pub(crate) use backend_esp_radio::{
    AccessPointInfo, AuthMethod, ClientConfig, ModeConfig, PowerSaveMode, Protocol, ScanConfig,
    ScanMethod, ScanTypeConfig, WifiMode,
};
pub(crate) use backend_owner::{
    backend_name, init_radio, initialize_runtime_sta, legacy_port_runtime_enabled, new_runtime,
    wifi_active_scan_config, wifi_channel_active_scan_config, wifi_client_mode_config,
    wifi_connect_async, wifi_directed_active_scan_config, wifi_disconnect_async,
    wifi_error_is_no_mem, wifi_is_started, wifi_passive_scan_config, wifi_power_save_none,
    wifi_raw_broad_scan_config, wifi_rssi, wifi_scan_with_config_async, wifi_set_config,
    wifi_set_mode, wifi_set_power_saving, wifi_set_protocol, wifi_sta_mode,
    wifi_standard_bgn_protocols, wifi_start_async, wifi_stop_async, wifi_runtime_config,
};
pub(crate) use esp_radio::wifi::event::{self, EventExt};
