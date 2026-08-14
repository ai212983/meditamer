use esp_hal::time::Duration;
use esp_radio::wifi::scan::ScanTypeConfig;

use super::ScanConfig;

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
