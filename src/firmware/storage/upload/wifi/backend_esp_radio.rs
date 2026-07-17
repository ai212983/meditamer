extern crate alloc;

use esp_hal::time::Duration;
use esp_radio::wifi::{
    scan::ScanTypeConfig,
    sta::{ScanMethod, StationConfig},
    AuthenticationMethod, Config, DisconnectReason, Interface, PowerSaveMode, Protocols,
};

pub(crate) use esp_radio::wifi::{
    ap::AccessPointInfo, scan::ScanConfig, ControllerConfig, WifiController, WifiError,
};

pub(crate) type WifiDriverConfig = ControllerConfig;
pub(crate) type WifiDevice = Interface;
pub(crate) type AuthMethod = AuthenticationMethod;
pub(crate) type ModeConfig = Config;

pub(crate) fn backend_name() -> &'static str {
    "esp-radio-1.0"
}

pub(crate) fn wifi_runtime_config(country_us_override: bool) -> WifiDriverConfig {
    // Instrumented debug code needs a deeper queue to sustain the accepted
    // throughput floor. Optimized release code drains two slots fast enough,
    // and the smaller bound preserves the internal-memory reserve.
    let rx_queue_size = if cfg!(debug_assertions) { 4 } else { 2 };
    let config = WifiDriverConfig::default().with_rx_queue_size(rx_queue_size);
    if country_us_override {
        config.with_country_info(esp_radio::wifi::CountryInfo::from(*b"US"))
    } else {
        config
    }
}

pub(crate) fn initialize_runtime_sta(
    wifi: esp_hal::peripherals::WIFI<'static>,
    country_us_override: bool,
) -> Result<(WifiController<'static>, WifiDevice), &'static str> {
    let sta = Interface::station();
    match WifiController::new(wifi, wifi_runtime_config(country_us_override)) {
        Ok(controller) => Ok((controller, sta)),
        Err(err) => {
            esp_println::println!("asset-upload-http: wifi init err={:?}", err);
            Err("asset-upload-http: wifi init failed")
        }
    }
}

pub(crate) fn wifi_set_config(
    controller: &mut WifiController<'_>,
    conf: &ModeConfig,
) -> Result<(), WifiError> {
    controller.set_config(conf)
}

// esp-radio 1.0 starts and changes mode through ControllerConfig/set_config.
// These compatibility operations keep the existing recovery state machine
// source-compatible without reaching into private driver lifecycle APIs.
pub(crate) fn wifi_set_mode(
    _controller: &mut WifiController<'_>,
    _mode: WifiMode,
) -> Result<(), WifiError> {
    Ok(())
}

pub(crate) fn wifi_is_started(_controller: &WifiController<'_>) -> Result<bool, WifiError> {
    Ok(true)
}

pub(crate) fn wifi_is_connected(controller: &WifiController<'_>) -> bool {
    controller.is_connected()
}

pub(crate) fn wifi_set_power_saving(
    controller: &mut WifiController<'_>,
    ps: PowerSaveMode,
) -> Result<(), WifiError> {
    controller.set_power_saving(ps)
}

pub(crate) fn wifi_set_protocol(
    controller: &mut WifiController<'_>,
    protocols: Protocols,
) -> Result<(), WifiError> {
    controller.set_protocols(protocols)
}

#[derive(Clone, Copy)]
pub(crate) struct WifiMode;

pub(crate) fn wifi_sta_mode() -> WifiMode {
    WifiMode
}

pub(crate) fn wifi_power_save_none() -> PowerSaveMode {
    PowerSaveMode::None
}

pub(crate) fn wifi_standard_bgn_protocols() -> Protocols {
    Protocols::default()
}

pub(crate) fn wifi_rssi(controller: &WifiController<'_>) -> Result<i32, WifiError> {
    controller.rssi()
}

pub(crate) fn wifi_error_is_no_mem(err: &WifiError) -> bool {
    matches!(err, WifiError::OutOfMemory)
}

pub(crate) fn wifi_client_mode_config(
    ssid: &str,
    password: &str,
    auth_method: AuthMethod,
    channel_hint: Option<u8>,
    bssid_hint: Option<[u8; 6]>,
) -> ModeConfig {
    let auth_method = if password.is_empty() {
        AuthMethod::None
    } else {
        auth_method
    };
    let scan_method = if channel_hint.is_some() {
        ScanMethod::Fast
    } else {
        ScanMethod::AllChannels
    };
    let mut station = StationConfig::default()
        .with_ssid(ssid)
        .with_password(password.into())
        .with_auth_method(auth_method)
        .with_scan_method(scan_method);
    if let Some(channel) = channel_hint {
        station = station.with_channel(channel);
    }
    if let Some(bssid) = bssid_hint {
        station = station.with_bssid(bssid);
    }
    ModeConfig::Station(station)
}

pub(crate) fn wifi_active_scan_config(max_results: usize, min_ms: u64, max_ms: u64) -> ScanConfig {
    ScanConfig::default()
        .with_show_hidden(true)
        .with_max(max_results)
        .with_scan_type(ScanTypeConfig::Active {
            min: Duration::from_millis(min_ms),
            max: Duration::from_millis(max_ms),
        })
}

pub(crate) fn wifi_directed_active_scan_config(
    ssid: &str,
    max_results: usize,
    min_ms: u64,
    max_ms: u64,
) -> ScanConfig {
    wifi_active_scan_config(max_results, min_ms, max_ms).with_ssid(ssid)
}

pub(crate) fn wifi_channel_active_scan_config(
    channel: u8,
    max_results: usize,
    min_ms: u64,
    max_ms: u64,
) -> ScanConfig {
    wifi_active_scan_config(max_results, min_ms, max_ms).with_channel(channel)
}

pub(crate) fn wifi_passive_scan_config(max_results: usize, passive_ms: u64) -> ScanConfig {
    ScanConfig::default()
        .with_show_hidden(true)
        .with_max(max_results)
        .with_scan_type(ScanTypeConfig::Passive(Duration::from_millis(passive_ms)))
}

pub(crate) fn wifi_raw_broad_scan_config(max_results: usize) -> ScanConfig {
    ScanConfig::default()
        .with_show_hidden(true)
        .with_max(max_results)
}

pub(crate) async fn wifi_scan_with_config_async(
    controller: &mut WifiController<'_>,
    config: ScanConfig,
) -> Result<alloc::vec::Vec<AccessPointInfo>, WifiError> {
    let result = controller.scan_async(&config).await;
    if let Ok(access_points) = &result {
        super::super::WIFI_LAST_SCAN_DONE_AT_MS.store(
            super::super::connect::monotonic_now_ms_u32(),
            core::sync::atomic::Ordering::Relaxed,
        );
        super::super::WIFI_LAST_SCAN_DONE_COUNT.store(
            access_points.len() as u32,
            core::sync::atomic::Ordering::Relaxed,
        );
        super::super::WIFI_LAST_SCAN_DONE_STATUS.store(0, core::sync::atomic::Ordering::Relaxed);
    }
    result
}

pub(crate) async fn wifi_start_async(
    _controller: &mut WifiController<'_>,
) -> Result<(), WifiError> {
    Ok(())
}

pub(crate) async fn wifi_stop_async(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    if controller.is_connected() {
        record_disconnect(controller.disconnect_async().await.map(|info| info.reason))?;
    }
    Ok(())
}

pub(crate) async fn wifi_connect_async(
    controller: &mut WifiController<'_>,
) -> Result<(), WifiError> {
    match controller.connect_async().await {
        Ok(_) => Ok(()),
        Err(err) => {
            record_disconnect_error(&err);
            Err(err)
        }
    }
}

pub(crate) async fn wifi_disconnect_async(
    controller: &mut WifiController<'_>,
) -> Result<(), WifiError> {
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
        Err(err) => {
            record_disconnect_error(&err);
            Err(err)
        }
    }
}

fn record_disconnect_error(err: &WifiError) {
    if let WifiError::Disconnected(info) = err {
        record_disconnect_reason(info.reason);
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
