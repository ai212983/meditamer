use embassy_time::Duration;
use esp_radio::wifi::{ScanConfig, ScanTypeConfig};

use crate::firmware::types::WifiRuntimePolicy;

pub(super) fn active_scan_config(target_ssid: &str, policy: WifiRuntimePolicy) -> ScanConfig<'_> {
    ScanConfig::default()
        .with_ssid(target_ssid)
        .with_show_hidden(true)
        .with_max(64)
        .with_scan_type(ScanTypeConfig::Active {
            min: Duration::from_millis(policy.scan_active_min_ms as u64).into(),
            max: Duration::from_millis(policy.scan_active_max_ms as u64).into(),
        })
}

pub(super) fn passive_scan_config(policy: WifiRuntimePolicy) -> ScanConfig<'static> {
    ScanConfig::default()
        .with_show_hidden(true)
        .with_max(64)
        .with_scan_type(ScanTypeConfig::Passive(
            Duration::from_millis(policy.scan_passive_ms as u64).into(),
        ))
}
