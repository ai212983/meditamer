use super::super::{
    backend_esp_radio, AuthMethod, ModeConfig, PowerSaveMode, Protocol, WifiController, WifiError,
    WifiMode,
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
