use alloc::vec::Vec;

use super::super::{AccessPointInfo, ScanConfig, WifiController, WifiError};

pub(crate) fn start(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    super::super::legacy_stack::control::start(controller)
}

pub(crate) fn stop(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    super::super::legacy_stack::control::stop(controller)
}

pub(crate) fn scan_with_config(
    controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<Vec<AccessPointInfo>, WifiError> {
    super::super::legacy_stack::control::scan_with_config(controller, config)
}
