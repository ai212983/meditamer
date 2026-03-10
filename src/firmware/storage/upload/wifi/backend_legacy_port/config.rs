extern crate alloc;

use crate::firmware::storage::upload::wifi::backend::{
    AuthMethod, ClientConfig, ModeConfig, PowerSaveMode, Protocol, ScanConfig, ScanMethod,
    ScanTypeConfig, WifiDriverConfig, WifiError, WifiMode,
};
use embassy_time::Duration;
use enumset::EnumSet;

fn idf_default_scan_timing_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_IDF_DEFAULT_SCAN_TIMING_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_IDF_DEFAULT_SCAN_TIMING_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

pub(crate) fn runtime_config(country_us_override: bool) -> WifiDriverConfig {
    if country_us_override {
        WifiDriverConfig::default().with_country_code(esp_radio::wifi::CountryInfo::from(*b"US"))
    } else {
        WifiDriverConfig::default()
    }
}

pub(crate) fn sta_mode() -> WifiMode {
    WifiMode::Sta
}

pub(crate) fn power_save_none() -> PowerSaveMode {
    PowerSaveMode::None
}

pub(crate) fn standard_bgn_protocols() -> EnumSet<Protocol> {
    Protocol::P802D11B | Protocol::P802D11BG | Protocol::P802D11BGN
}

pub(crate) fn error_is_no_mem(err: &WifiError) -> bool {
    matches!(
        err,
        WifiError::InternalError(esp_radio::wifi::InternalWifiError::NoMem)
    )
}

pub(crate) fn client_mode_config(
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
    let mut client = ClientConfig::default()
        .with_ssid(ssid.into())
        .with_password(password.into())
        .with_auth_method(auth_method)
        .with_scan_method(scan_method);
    if let Some(channel) = channel_hint {
        client = client.with_channel(channel);
    }
    if let Some(bssid) = bssid_hint {
        client = client.with_bssid(bssid);
    }
    ModeConfig::Client(client)
}

pub(crate) fn active_scan_config(
    max_results: usize,
    min_ms: u64,
    max_ms: u64,
) -> ScanConfig<'static> {
    ScanConfig::default()
        .with_show_hidden(true)
        .with_max(max_results)
        .with_scan_type(ScanTypeConfig::Active {
            min: Duration::from_millis(min_ms).into(),
            max: Duration::from_millis(max_ms).into(),
        })
}

pub(crate) fn directed_active_scan_config(
    ssid: &str,
    max_results: usize,
    min_ms: u64,
    max_ms: u64,
) -> ScanConfig<'_> {
    active_scan_config(max_results, min_ms, max_ms).with_ssid(ssid)
}

pub(crate) fn channel_active_scan_config(
    channel: u8,
    max_results: usize,
    min_ms: u64,
    max_ms: u64,
) -> ScanConfig<'static> {
    active_scan_config(max_results, min_ms, max_ms).with_channel(channel)
}

pub(crate) fn passive_scan_config(max_results: usize, passive_ms: u64) -> ScanConfig<'static> {
    ScanConfig::default()
        .with_show_hidden(true)
        .with_max(max_results)
        .with_scan_type(ScanTypeConfig::Passive(
            Duration::from_millis(passive_ms).into(),
        ))
}

pub(crate) fn raw_broad_scan_config(max_results: usize) -> ScanConfig<'static> {
    if idf_default_scan_timing_enabled() {
        active_scan_config(max_results, 0, 120)
    } else {
        active_scan_config(max_results, 10, 20)
    }
}
