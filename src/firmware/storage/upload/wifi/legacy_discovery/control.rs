extern crate alloc;

use super::super::{
    wifi_raw_broad_scan_config, wifi_scan_with_config_async, wifi_stop_async, ScanConfig,
    WifiError,
};
use super::{LegacyDiscoveryResult, LegacyDiscoverySession};

pub(crate) async fn scan_broad(
    session: &mut LegacyDiscoverySession<'_>,
    max_results: usize,
) -> Result<LegacyDiscoveryResult, WifiError> {
    let config = wifi_raw_broad_scan_config(max_results);
    scan_with_config(session, config).await
}

pub(crate) async fn scan_with_config(
    session: &mut LegacyDiscoverySession<'_>,
    config: ScanConfig<'_>,
) -> Result<LegacyDiscoveryResult, WifiError> {
    wifi_scan_with_config_async(session.controller, config).await
}

pub(crate) async fn shutdown(session: LegacyDiscoverySession<'_>) -> Result<(), WifiError> {
    if session.owns_start {
        wifi_stop_async(session.controller).await
    } else {
        Ok(())
    }
}
