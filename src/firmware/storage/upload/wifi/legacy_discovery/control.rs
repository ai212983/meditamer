extern crate alloc;

use super::super::{
    backend_legacy_port, ScanConfig, WifiController, WifiError,
};
use super::{LegacyDiscoveryResult, LegacyDiscoverySession};

pub(crate) async fn scan_broad(
    session: &mut LegacyDiscoverySession<'_>,
    max_results: usize,
) -> Result<LegacyDiscoveryResult, WifiError> {
    let config = backend_legacy_port::legacy_raw_broad_scan_config(max_results);
    scan_with_config(session, config).await
}

pub(crate) async fn scan_with_config(
    session: &mut LegacyDiscoverySession<'_>,
    config: ScanConfig<'_>,
) -> Result<LegacyDiscoveryResult, WifiError> {
    scan_with_controller(session.controller, config).await
}

pub(crate) async fn scan_with_controller(
    controller: &mut WifiController<'static>,
    config: ScanConfig<'_>,
) -> Result<LegacyDiscoveryResult, WifiError> {
    backend_legacy_port::controller_scan_with_config(controller, config).await
}

pub(crate) async fn shutdown(session: LegacyDiscoverySession<'_>) -> Result<(), WifiError> {
    if session.owns_start {
        backend_legacy_port::controller_stop(session.controller).await
    } else {
        Ok(())
    }
}
