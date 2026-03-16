use alloc::vec::Vec;

use super::super::{AccessPointInfo, Config, Interfaces, ScanConfig, WifiController, WifiError};

pub(crate) fn wifi_new<'d>(
    device: crate::hal::peripherals::WIFI<'d>,
    config: Config,
) -> Result<(WifiController<'d>, Interfaces<'d>), WifiError> {
    super::super::legacy_stack::init::wifi_new(device, config)
}

pub(crate) fn start(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    super::super::legacy_stack::init::start(controller)
}

pub(crate) fn stop(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    super::super::legacy_stack::init::stop(controller)
}

pub(crate) fn scan_with_config(
    controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<Vec<AccessPointInfo>, WifiError> {
    super::super::legacy_stack::init::scan_with_config(controller, config)
}
