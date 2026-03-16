use core::task::Context;

use super::super::{WifiDeviceMode, WifiRxToken, WifiTxToken};

pub(crate) fn tx_can_send() -> bool {
    super::super::legacy_stack::rx::tx_can_send()
}

pub(crate) fn increase_tx_inflight() {
    super::super::legacy_stack::rx::increase_tx_inflight()
}

pub(crate) fn tx_token(mode: WifiDeviceMode) -> Option<WifiTxToken> {
    super::super::legacy_stack::rx::tx_token(mode)
}

pub(crate) fn rx_token(mode: WifiDeviceMode, can_send: bool) -> Option<(WifiRxToken, WifiTxToken)> {
    super::super::legacy_stack::rx::rx_token(mode, can_send)
}

pub(crate) fn register_receive_waker(mode: WifiDeviceMode, cx: &mut Context<'_>) {
    super::super::legacy_stack::rx::register_receive_waker(mode, cx)
}

pub(crate) fn consume_rx_token<R, F>(mode: WifiDeviceMode, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    super::super::legacy_stack::rx::consume_rx_token(mode, f)
}

pub(crate) fn consume_tx_token<R, F>(mode: WifiDeviceMode, len: usize, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    super::super::legacy_stack::rx::consume_tx_token(mode, len, f)
}

pub(crate) unsafe extern "C" fn recv_cb_sta(
    buffer: *mut crate::binary::c_types::c_void,
    len: u16,
    eb: *mut crate::binary::c_types::c_void,
) -> i32 {
    unsafe { super::super::legacy_stack::rx::recv_cb_sta(buffer, len, eb) }
}

pub(crate) unsafe extern "C" fn recv_cb_ap(
    buffer: *mut crate::binary::c_types::c_void,
    len: u16,
    eb: *mut crate::binary::c_types::c_void,
) -> i32 {
    unsafe { super::super::legacy_stack::rx::recv_cb_ap(buffer, len, eb) }
}
