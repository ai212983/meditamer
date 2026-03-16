use super::super::{driver, ScanConfig};
use crate::firmware::types::WifiRuntimePolicy;

pub(crate) fn active_scan_config(policy: WifiRuntimePolicy) -> ScanConfig<'static> {
    driver::active_scan_config(policy)
}

pub(crate) fn directed_active_scan_config(
    target_ssid: &str,
    policy: WifiRuntimePolicy,
) -> ScanConfig<'_> {
    driver::directed_active_scan_config(target_ssid, policy)
}

pub(crate) fn channel_active_scan_config(
    channel: u8,
    policy: WifiRuntimePolicy,
) -> ScanConfig<'static> {
    driver::channel_active_scan_config(channel, policy)
}

pub(crate) fn passive_scan_config(policy: WifiRuntimePolicy) -> ScanConfig<'static> {
    driver::passive_scan_config(policy)
}
