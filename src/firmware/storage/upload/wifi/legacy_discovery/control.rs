extern crate alloc;

use super::super::{
    wifi_raw_broad_scan_config, wifi_scan_with_config_async, ScanConfig, WifiController, WifiError,
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
    config: ScanConfig,
) -> Result<LegacyDiscoveryResult, WifiError> {
    scan_with_controller(session.controller, config).await
}

pub(crate) async fn scan_with_controller(
    controller: &mut WifiController<'static>,
    config: ScanConfig,
) -> Result<LegacyDiscoveryResult, WifiError> {
    wifi_scan_with_config_async(controller, config).await
}

pub(crate) async fn shutdown(session: LegacyDiscoverySession<'_>) -> Result<(), WifiError> {
    let _ = session;
    Ok(())
}
