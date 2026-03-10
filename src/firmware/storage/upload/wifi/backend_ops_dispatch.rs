extern crate alloc;

use super::{
    backend_esp_radio, AccessPointInfo, AuthMethod, ModeConfig, PowerSaveMode, Protocol,
    ScanConfig, WifiController, WifiError, WifiMode,
};
use crate::firmware::storage::upload::wifi::backend_legacy_port;
use enumset::EnumSet;

fn use_legacy_port() -> bool {
    backend_legacy_port::legacy_port_runtime_enabled()
}

pub(crate) fn wifi_set_config(
    controller: &mut WifiController<'_>,
    conf: &ModeConfig,
) -> Result<(), WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_set_config(controller, conf)
    } else {
        backend_esp_radio::wifi_set_config(controller, conf)
    }
}

pub(crate) fn wifi_set_mode(
    controller: &mut WifiController<'_>,
    mode: WifiMode,
) -> Result<(), WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_set_mode(controller, mode)
    } else {
        backend_esp_radio::wifi_set_mode(controller, mode)
    }
}

pub(crate) fn wifi_is_started(controller: &WifiController<'_>) -> Result<bool, WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_is_started(controller)
    } else {
        backend_esp_radio::wifi_is_started(controller)
    }
}

pub(crate) fn wifi_set_power_saving(
    controller: &mut WifiController<'_>,
    ps: PowerSaveMode,
) -> Result<(), WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_set_power_saving(controller, ps)
    } else {
        backend_esp_radio::wifi_set_power_saving(controller, ps)
    }
}

pub(crate) fn wifi_set_protocol(
    controller: &mut WifiController<'_>,
    protocols: EnumSet<Protocol>,
) -> Result<(), WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_set_protocol(controller, protocols)
    } else {
        backend_esp_radio::wifi_set_protocol(controller, protocols)
    }
}

pub(crate) fn wifi_sta_mode() -> WifiMode {
    if use_legacy_port() {
        backend_legacy_port::legacy_sta_mode()
    } else {
        backend_esp_radio::wifi_sta_mode()
    }
}

pub(crate) fn wifi_power_save_none() -> PowerSaveMode {
    if use_legacy_port() {
        backend_legacy_port::legacy_power_save_none()
    } else {
        backend_esp_radio::wifi_power_save_none()
    }
}

pub(crate) fn wifi_standard_bgn_protocols() -> EnumSet<Protocol> {
    if use_legacy_port() {
        backend_legacy_port::legacy_standard_bgn_protocols()
    } else {
        backend_esp_radio::wifi_standard_bgn_protocols()
    }
}

pub(crate) fn wifi_rssi(controller: &WifiController<'_>) -> Result<i32, WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_rssi(controller)
    } else {
        backend_esp_radio::wifi_rssi(controller)
    }
}

pub(crate) fn wifi_error_is_no_mem(err: &WifiError) -> bool {
    if use_legacy_port() {
        backend_legacy_port::legacy_error_is_no_mem(err)
    } else {
        backend_esp_radio::wifi_error_is_no_mem(err)
    }
}

pub(crate) fn wifi_client_mode_config(
    ssid: &str,
    password: &str,
    auth_method: AuthMethod,
    channel_hint: Option<u8>,
    bssid_hint: Option<[u8; 6]>,
) -> ModeConfig {
    if use_legacy_port() {
        backend_legacy_port::legacy_client_mode_config(
            ssid,
            password,
            auth_method,
            channel_hint,
            bssid_hint,
        )
    } else {
        backend_esp_radio::wifi_client_mode_config(
            ssid,
            password,
            auth_method,
            channel_hint,
            bssid_hint,
        )
    }
}

pub(crate) fn wifi_active_scan_config(
    max_results: usize,
    min_ms: u64,
    max_ms: u64,
) -> ScanConfig<'static> {
    if use_legacy_port() {
        backend_legacy_port::legacy_active_scan_config(max_results, min_ms, max_ms)
    } else {
        backend_esp_radio::wifi_active_scan_config(max_results, min_ms, max_ms)
    }
}

pub(crate) fn wifi_directed_active_scan_config(
    ssid: &str,
    max_results: usize,
    min_ms: u64,
    max_ms: u64,
) -> ScanConfig<'_> {
    if use_legacy_port() {
        backend_legacy_port::legacy_directed_active_scan_config(ssid, max_results, min_ms, max_ms)
    } else {
        backend_esp_radio::wifi_directed_active_scan_config(ssid, max_results, min_ms, max_ms)
    }
}

pub(crate) fn wifi_channel_active_scan_config(
    channel: u8,
    max_results: usize,
    min_ms: u64,
    max_ms: u64,
) -> ScanConfig<'static> {
    if use_legacy_port() {
        backend_legacy_port::legacy_channel_active_scan_config(channel, max_results, min_ms, max_ms)
    } else {
        backend_esp_radio::wifi_channel_active_scan_config(channel, max_results, min_ms, max_ms)
    }
}

pub(crate) fn wifi_passive_scan_config(max_results: usize, passive_ms: u64) -> ScanConfig<'static> {
    if use_legacy_port() {
        backend_legacy_port::legacy_passive_scan_config(max_results, passive_ms)
    } else {
        backend_esp_radio::wifi_passive_scan_config(max_results, passive_ms)
    }
}

pub(crate) fn wifi_raw_broad_scan_config(max_results: usize) -> ScanConfig<'static> {
    if use_legacy_port() {
        backend_legacy_port::legacy_raw_broad_scan_config(max_results)
    } else {
        backend_esp_radio::wifi_raw_broad_scan_config(max_results)
    }
}

pub(crate) async fn wifi_scan_with_config_async(
    controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<alloc::vec::Vec<AccessPointInfo>, WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_scan_with_config(controller, config).await
    } else {
        backend_esp_radio::wifi_scan_with_config_async(controller, config).await
    }
}

pub(crate) async fn wifi_start_async(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_start(controller).await
    } else {
        backend_esp_radio::wifi_start_async(controller).await
    }
}

pub(crate) async fn wifi_stop_async(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_stop(controller).await
    } else {
        backend_esp_radio::wifi_stop_async(controller).await
    }
}

pub(crate) async fn wifi_connect_async(
    controller: &mut WifiController<'_>,
) -> Result<(), WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_connect(controller).await
    } else {
        backend_esp_radio::wifi_connect_async(controller).await
    }
}

pub(crate) async fn wifi_disconnect_async(
    controller: &mut WifiController<'_>,
) -> Result<(), WifiError> {
    if use_legacy_port() {
        backend_legacy_port::controller_disconnect(controller).await
    } else {
        backend_esp_radio::wifi_disconnect_async(controller).await
    }
}
