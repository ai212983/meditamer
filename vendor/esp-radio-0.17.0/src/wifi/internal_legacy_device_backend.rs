use super::{
    internal_legacy_packet_backend::EspWifiPacketBuffer,
    WifiDeviceMode,
    WifiRxToken,
    WifiTxToken,
    dump_packet_info,
    embassy,
    esp_wifi_send_data,
    internal_legacy_packet_backend,
};

pub(crate) fn tx_token(mode: WifiDeviceMode) -> Option<WifiTxToken> {
    if internal_legacy_packet_backend::tx_token_ready() {
        Some(WifiTxToken { mode })
    } else {
        None
    }
}

pub(crate) fn rx_token(mode: WifiDeviceMode, can_send: bool) -> Option<(WifiRxToken, WifiTxToken)> {
    if internal_legacy_packet_backend::rx_token_ready(mode, can_send) {
        tx_token(mode).map(|tx| (WifiRxToken { mode }, tx))
    } else {
        None
    }
}

pub(crate) fn consume_rx_token<R, F>(mode: WifiDeviceMode, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    let mut data: EspWifiPacketBuffer = internal_legacy_packet_backend::pop_rx_packet(mode);
    let buffer = data.as_slice_mut();
    dump_packet_info(buffer);
    f(buffer)
}

pub(crate) fn consume_tx_token<R, F>(mode: WifiDeviceMode, len: usize, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    internal_legacy_packet_backend::increase_tx_inflight();

    static mut BUFFER: [u8; super::MTU] = [0u8; super::MTU];
    let buffer = unsafe { &mut BUFFER[..len] };
    let res = f(buffer);
    esp_wifi_send_data(mode.interface(), buffer);
    res
}

pub(crate) fn register_receive_waker(mode: WifiDeviceMode, cx: &mut core::task::Context<'_>) {
    match mode {
        WifiDeviceMode::Sta => embassy::STA_RECEIVE_WAKER.register(cx.waker()),
        WifiDeviceMode::Ap => embassy::AP_RECEIVE_WAKER.register(cx.waker()),
    }
}
