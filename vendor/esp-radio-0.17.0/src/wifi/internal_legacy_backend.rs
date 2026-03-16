use alloc::vec::Vec;
use core::task::Context;

#[cfg(all(feature = "sniffer", feature = "unstable"))]
use crate::wifi::PromiscuousPkt;

use super::{
    internal_legacy_admission_literal,
    internal_legacy_device_backend,
    internal_legacy_packet_backend,
    AccessPointInfo,
    ScanConfig,
    WifiController,
    WifiDeviceMode,
    WifiError,
    WifiRxToken,
    WifiTxToken,
};

pub(crate) fn enabled() -> bool {
    internal_legacy_packet_backend::enabled()
}

pub(crate) fn start(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    internal_legacy_admission_literal::start(controller)
}

pub(crate) fn stop(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    internal_legacy_admission_literal::stop(controller)
}

pub(crate) fn scan_with_config(
    controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<Vec<AccessPointInfo>, WifiError> {
    internal_legacy_admission_literal::scan_with_config(controller, config)
}

pub(crate) fn tx_can_send() -> bool {
    internal_legacy_packet_backend::tx_can_send()
}

pub(crate) fn increase_tx_inflight() {
    internal_legacy_packet_backend::increase_tx_inflight();
}

pub(crate) fn tx_token(mode: WifiDeviceMode) -> Option<WifiTxToken> {
    internal_legacy_device_backend::tx_token(mode)
}

pub(crate) fn rx_token(mode: WifiDeviceMode, can_send: bool) -> Option<(WifiRxToken, WifiTxToken)> {
    internal_legacy_device_backend::rx_token(mode, can_send)
}

pub(crate) fn register_receive_waker(mode: WifiDeviceMode, cx: &mut Context<'_>) {
    internal_legacy_device_backend::register_receive_waker(mode, cx);
}

pub(crate) fn consume_rx_token<R, F>(mode: WifiDeviceMode, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    internal_legacy_device_backend::consume_rx_token(mode, f)
}

pub(crate) fn consume_tx_token<R, F>(mode: WifiDeviceMode, len: usize, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    internal_legacy_device_backend::consume_tx_token(mode, len, f)
}

pub(crate) unsafe extern "C" fn recv_cb_sta(
    buffer: *mut crate::binary::c_types::c_void,
    len: u16,
    eb: *mut crate::binary::c_types::c_void,
) -> i32 {
    unsafe { internal_legacy_packet_backend::recv_cb_sta(buffer, len, eb) }
}

pub(crate) unsafe extern "C" fn recv_cb_ap(
    buffer: *mut crate::binary::c_types::c_void,
    len: u16,
    eb: *mut crate::binary::c_types::c_void,
) -> i32 {
    unsafe { internal_legacy_packet_backend::recv_cb_ap(buffer, len, eb) }
}

#[cfg(all(feature = "sniffer", feature = "unstable"))]
pub(crate) unsafe extern "C" fn promiscuous_rx_cb(
    buf: *mut core::ffi::c_void,
    frame_type: u32,
) {
    unsafe { internal_legacy_packet_backend::promiscuous_rx_cb(buf, frame_type) }
}

#[cfg(all(feature = "sniffer", feature = "unstable"))]
pub(crate) fn sniffer_set(cb: fn(PromiscuousPkt<'_>)) {
    internal_legacy_packet_backend::sniffer_set(cb);
}
