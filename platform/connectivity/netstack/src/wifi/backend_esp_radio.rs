extern crate alloc;

use core::sync::atomic::{AtomicBool, Ordering};
use esp_radio::wifi::{
    sta::{ScanMethod, StationConfig},
    AuthenticationMethod, AuthenticationMethodConfig, Config, ConnectionError, DisconnectReason,
    Interface, Password, PowerSaveMode, Ssid,
};

#[path = "backend_esp_radio/scan_config.rs"]
mod scan_config;

pub use scan_config::{
    wifi_active_scan_config, wifi_channel_active_scan_config, wifi_directed_active_scan_config,
    wifi_passive_scan_config, wifi_raw_broad_scan_config,
};

pub use esp_radio::wifi::{
    ap::AccessPointInfo, scan::ScanConfig, ControllerConfig, WifiController, WifiError,
};

pub type WifiDriverConfig = ControllerConfig;
pub type WifiDevice = Interface;
pub type AuthMethod = AuthenticationMethod;
pub type ModeConfig = Config;

// `WifiController::new` starts the configured interface, but esp-radio 1.0 beta
// does not expose its internal Started/Stopped state. Keep the state at this
// adapter boundary so the recovery machine can make truthful start/stop
// decisions after using the raw lifecycle calls below.
static WIFI_STARTED: AtomicBool = AtomicBool::new(false);

pub fn backend_name() -> &'static str {
    "esp-radio-1.0"
}

pub fn wifi_runtime_config() -> WifiDriverConfig {
    // Keep the calibrated product queue shape and vendor AMPDU receive default.
    // E-0037's single BLE-release-only AMPDU-off falsification still reached a
    // 13,376-byte serving low-water in the binding ten-cycle soak, so the
    // experiment is closed as insufficient and must not remain in production.
    let rx_queue_size = if cfg!(debug_assertions) { 4 } else { 2 };
    WifiDriverConfig::default().with_rx_queue_size(rx_queue_size)
}

pub fn initialize_runtime_sta<'d>(
    wifi: esp_hal::peripherals::WIFI<'d>,
) -> Result<(WifiController<'d>, WifiDevice), &'static str> {
    let sta = Interface::station();
    match WifiController::new(wifi, wifi_runtime_config()) {
        Ok(controller) => {
            WIFI_STARTED.store(true, Ordering::Release);
            Ok((controller, sta))
        }
        Err(err) => {
            WIFI_STARTED.store(false, Ordering::Release);
            console::println!("asset-upload-http: wifi init err={:?}", err);
            if matches!(err, WifiError::CleanupFailed) {
                Err("asset-upload-http: wifi ownership unknown")
            } else {
                Err("asset-upload-http: wifi init failed")
            }
        }
    }
}

pub fn wifi_set_config(
    controller: &mut WifiController<'_>,
    conf: &ModeConfig,
) -> Result<(), WifiError> {
    controller.set_config(conf)
}

pub fn wifi_is_started(_controller: &WifiController<'_>) -> Result<bool, WifiError> {
    Ok(WIFI_STARTED.load(Ordering::Acquire))
}

pub fn wifi_is_connected(controller: &WifiController<'_>) -> bool {
    controller.is_connected()
}

pub fn wifi_set_power_saving(
    controller: &mut WifiController<'_>,
    ps: PowerSaveMode,
) -> Result<(), WifiError> {
    controller.set_power_saving(ps)
}

pub fn wifi_power_save_none() -> PowerSaveMode {
    PowerSaveMode::None
}

pub fn wifi_rssi(controller: &WifiController<'_>) -> Result<i32, WifiError> {
    controller.rssi()
}

pub fn wifi_error_is_no_mem(err: &WifiError) -> bool {
    matches!(err, WifiError::OutOfMemory)
}

pub fn wifi_client_mode_config(
    ssid: &str,
    password: &str,
    auth_method: AuthMethod,
    channel_hint: Option<u8>,
    bssid_hint: Option<[u8; 6]>,
) -> Option<ModeConfig> {
    // esp-radio 1.0.0-beta.1 rejects oversized SSIDs/passwords instead of
    // truncating them; unrepresentable credentials fail config construction.
    let ssid = Ssid::try_from(ssid).ok()?;
    let open = password.is_empty();
    let password = Password::try_from(password).ok()?;
    let authentication = if open {
        AuthenticationMethodConfig::Open
    } else {
        match auth_method {
            AuthMethod::None => AuthenticationMethodConfig::Open,
            AuthMethod::Wep => AuthenticationMethodConfig::Wep(password),
            AuthMethod::Wpa => AuthenticationMethodConfig::Wpa(password),
            AuthMethod::Wpa2Personal => AuthenticationMethodConfig::Wpa2Personal(password),
            AuthMethod::WpaWpa2Personal => AuthenticationMethodConfig::WpaWpa2Personal(password),
            _ => AuthenticationMethodConfig::WpaWpa2Personal(password),
        }
    };
    let scan_method = if channel_hint.is_some() {
        ScanMethod::Fast
    } else {
        ScanMethod::AllChannels
    };
    let mut station = StationConfig::default()
        .with_ssid(ssid)
        .with_authentication(authentication)
        .with_scan_method(scan_method);
    if let Some(channel) = channel_hint {
        station = station.with_channel(channel);
    }
    if let Some(bssid) = bssid_hint {
        station = station.with_bssid(bssid);
    }
    Some(ModeConfig::Station(station))
}

pub async fn wifi_scan_with_config_async(
    controller: &mut WifiController<'_>,
    config: ScanConfig,
) -> Result<alloc::vec::Vec<AccessPointInfo>, WifiError> {
    controller.scan_async(&config).await
}

pub async fn wifi_start_async(_controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    if WIFI_STARTED.load(Ordering::Acquire) {
        return Ok(());
    }

    // SAFETY: the network owner holds the sole live controller and has not
    // started this initialized driver instance yet.
    let result = unsafe { esp_wifi_sys::include::esp_wifi_start() };
    match result as u32 {
        esp_wifi_sys::include::ESP_OK => {
            WIFI_STARTED.store(true, Ordering::Release);
            Ok(())
        }
        esp_wifi_sys::include::ESP_ERR_NO_MEM => Err(WifiError::OutOfMemory),
        esp_wifi_sys::include::ESP_ERR_INVALID_ARG => Err(WifiError::InvalidArguments),
        _ => Err(WifiError::Other),
    }
}

pub async fn wifi_stop_async(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    if controller.is_connected() {
        record_disconnect(controller.disconnect_async().await.map(|info| info.reason))?;
    }

    if !WIFI_STARTED.load(Ordering::Acquire) {
        return Ok(());
    }

    // SAFETY: the network owner holds the sole live controller and has
    // disconnected it before stopping the initialized driver instance.
    let result = unsafe { esp_wifi_sys::include::esp_wifi_stop() };
    match result as u32 {
        esp_wifi_sys::include::ESP_OK | esp_wifi_sys::include::ESP_ERR_WIFI_NOT_STARTED => {
            WIFI_STARTED.store(false, Ordering::Release);
            Ok(())
        }
        _ => Err(WifiError::Other),
    }
}

pub fn wifi_shutdown_source(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    controller.shutdown_source()
}

pub fn wifi_finalize_shutdown(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    controller.finalize_shutdown()
}

pub fn wifi_callback_stats() -> esp_radio::wifi::WifiCallbackStats {
    esp_radio::wifi::wifi_callback_stats()
}

pub fn wifi_rx_buffer_stats() -> esp_radio::wifi::WifiRxBufferStats {
    esp_radio::wifi::wifi_rx_buffer_stats()
}

pub async fn wifi_connect_async(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    match controller.connect_async().await {
        Ok(_) => Ok(()),
        // esp-radio 1.0.0-beta.1 reports connection loss as
        // `ConnectionError::Failed(info)` instead of `WifiError::Disconnected`;
        // the disconnect reason is still recorded, then surfaced opaquely.
        Err(ConnectionError::Failed(info)) => {
            record_disconnect_reason(info.reason);
            Err(WifiError::Other)
        }
        Err(ConnectionError::WifiError(err)) => Err(err),
        // `ConnectionError` is non-exhaustive; future variants surface opaquely.
        Err(_) => Err(WifiError::Other),
    }
}

pub async fn wifi_disconnect_async(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    if !controller.is_connected() {
        return Ok(());
    }
    record_disconnect(controller.disconnect_async().await.map(|info| info.reason))
}

fn record_disconnect(result: Result<DisconnectReason, WifiError>) -> Result<(), WifiError> {
    match result {
        Ok(reason) => {
            record_disconnect_reason(reason);
            Ok(())
        }
        // No remaining `WifiError` variant carries a disconnect reason (beta.1
        // moved it to `ConnectionError::Failed`), so there is nothing to record.
        Err(err) => Err(err),
    }
}

fn record_disconnect_reason(reason: DisconnectReason) {
    let reason = match reason {
        DisconnectReason::AuthenticationExpired => 2,
        DisconnectReason::FourWayHandshakeTimeout => 15,
        DisconnectReason::BeaconTimeout => 200,
        DisconnectReason::NoAccessPointFound => 201,
        DisconnectReason::AuthenticationFailed => 202,
        DisconnectReason::AssociationFailed => 203,
        DisconnectReason::HandshakeTimeout => 204,
        DisconnectReason::ConnectionFailed => 205,
        DisconnectReason::NoAccessPointFoundWithCompatibleSecurity => 210,
        DisconnectReason::NoAccessPointFoundInAuthmodeThreshold => 211,
        DisconnectReason::NoAccessPointFoundInRssiThreshold => 212,
        _ => 1,
    };
    super::super::WIFI_LAST_DISCONNECT_REASON.store(reason, core::sync::atomic::Ordering::Relaxed);
    super::super::WIFI_DISCONNECTED_EVENT.store(true, core::sync::atomic::Ordering::Relaxed);
}
