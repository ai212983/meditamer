extern crate alloc;

use super::super::{
    backend_esp_radio, AccessPointInfo, ScanConfig, WifiController, WifiError,
};
use crate::firmware::storage::upload::wifi::backend_legacy_port;

fn use_legacy_port() -> bool {
    backend_legacy_port::legacy_port_runtime_enabled()
}

pub(crate) fn wifi_error_is_no_mem(err: &WifiError) -> bool {
    if use_legacy_port() {
        backend_legacy_port::legacy_error_is_no_mem(err)
    } else {
        backend_esp_radio::wifi_error_is_no_mem(err)
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
