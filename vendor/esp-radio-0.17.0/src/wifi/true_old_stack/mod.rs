pub(crate) mod control;
pub(crate) mod init;
pub(crate) mod install;
pub(crate) mod rx;

use alloc::vec::Vec;
use core::task::Context;

use super::{
    AccessPointInfo,
    Config,
    Interfaces,
    ScanConfig,
    WifiController,
    WifiDeviceMode,
    WifiError,
    WifiRxToken,
    WifiTxToken,
};

pub(crate) fn wifi_new<'d>(
    device: crate::hal::peripherals::WIFI<'d>,
    config: Config,
) -> Result<(WifiController<'d>, Interfaces<'d>), WifiError> {
    init::wifi_new(device, config)
}

pub(crate) fn start(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    control::start(controller)
}

pub(crate) fn stop(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    control::stop(controller)
}

pub(crate) fn scan_with_config(
    controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<Vec<AccessPointInfo>, WifiError> {
    control::scan_with_config(controller, config)
}

pub(crate) fn tx_can_send() -> bool {
    rx::tx_can_send()
}

pub(crate) fn increase_tx_inflight() {
    rx::increase_tx_inflight()
}

pub(crate) fn tx_token(mode: WifiDeviceMode) -> Option<WifiTxToken> {
    rx::tx_token(mode)
}

pub(crate) fn rx_token(mode: WifiDeviceMode, can_send: bool) -> Option<(WifiRxToken, WifiTxToken)> {
    rx::rx_token(mode, can_send)
}

pub(crate) fn register_receive_waker(mode: WifiDeviceMode, cx: &mut Context<'_>) {
    rx::register_receive_waker(mode, cx)
}

pub(crate) fn consume_rx_token<R, F>(mode: WifiDeviceMode, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    rx::consume_rx_token(mode, f)
}

pub(crate) fn consume_tx_token<R, F>(mode: WifiDeviceMode, len: usize, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    rx::consume_tx_token(mode, len, f)
}
