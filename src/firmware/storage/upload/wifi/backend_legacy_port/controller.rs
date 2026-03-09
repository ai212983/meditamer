extern crate alloc;

use super::{AccessPointInfo, ScanConfig, WifiController, WifiError};

pub(crate) async fn scan_with_config(
    controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<alloc::vec::Vec<AccessPointInfo>, WifiError> {
    esp_radio::wifi::WifiController::scan_with_config(controller, config)
}

pub(crate) async fn start(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::start(controller)
}

pub(crate) async fn stop(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_radio::wifi::WifiController::stop(controller)
}
