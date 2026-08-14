extern crate alloc;

use super::super::{
    wifi_scan_with_config_async, AccessPointInfo, ScanConfig, WifiController, WifiError,
};

pub(crate) async fn scan_with_controller(
    controller: &mut WifiController<'_>,
    config: ScanConfig,
) -> Result<alloc::vec::Vec<AccessPointInfo>, WifiError> {
    wifi_scan_with_config_async(controller, config).await
}
