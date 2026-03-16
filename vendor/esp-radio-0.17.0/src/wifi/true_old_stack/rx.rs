use alloc::collections::vec_deque::VecDeque;
use core::{
    cell::UnsafeCell,
    sync::atomic::Ordering,
    task::Context,
};

#[cfg(all(feature = "sniffer", feature = "unstable"))]
use crate::wifi::PromiscuousPkt;

use super::super::{
    WIFI_RX_CB_AP_COUNT,
    WIFI_RX_CB_STA_COUNT,
    WIFI_TX_INFLIGHT,
    RX_QUEUE_SIZE,
    TX_QUEUE_SIZE,
    WifiDeviceMode,
    WifiRxToken,
    WifiTxToken,
    dump_packet_info,
    embassy,
    esp_wifi_send_data,
};
use crate::{
    binary::{c_types::c_void, include},
    compat::legacy_runtime_policy::backend_legacy_port_enabled,
};

struct Locked<T> {
    inner: UnsafeCell<T>,
}

unsafe impl<T> Sync for Locked<T> {}

impl<T> Locked<T> {
    const fn new(value: T) -> Self {
        Self {
            inner: UnsafeCell::new(value),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        critical_section::with(|_| f(unsafe { &mut *self.inner.get() }))
    }
}

#[derive(Debug)]
pub(crate) struct EspWifiPacketBuffer {
    pub(crate) buffer: *mut c_void,
    pub(crate) len: u16,
    pub(crate) eb: *mut c_void,
}

unsafe impl Send for EspWifiPacketBuffer {}

impl Drop for EspWifiPacketBuffer {
    fn drop(&mut self) {
        unsafe { include::esp_wifi_internal_free_rx_buffer(self.eb) };
    }
}

impl EspWifiPacketBuffer {
    pub(crate) fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.buffer.cast::<u8>(), self.len as usize) }
    }
}

static DATA_QUEUE_RX_AP: Locked<VecDeque<EspWifiPacketBuffer>> = Locked::new(VecDeque::new());
static DATA_QUEUE_RX_STA: Locked<VecDeque<EspWifiPacketBuffer>> = Locked::new(VecDeque::new());

#[cfg(all(feature = "sniffer", feature = "unstable"))]
static SNIFFER_CB: Locked<Option<fn(PromiscuousPkt<'_>)>> = Locked::new(None);

pub(crate) fn enabled() -> bool {
    backend_legacy_port_enabled()
}

fn queue_for(mode: WifiDeviceMode) -> &'static Locked<VecDeque<EspWifiPacketBuffer>> {
    match mode {
        WifiDeviceMode::Sta => &DATA_QUEUE_RX_STA,
        WifiDeviceMode::Ap => &DATA_QUEUE_RX_AP,
    }
}

fn enqueue_rx_packet(
    mode: WifiDeviceMode,
    packet: EspWifiPacketBuffer,
    rx_queue_size: usize,
) -> Result<(), EspWifiPacketBuffer> {
    queue_for(mode).with(|queue| {
        if queue.len() < rx_queue_size {
            queue.push_back(packet);
            Ok(())
        } else {
            Err(packet)
        }
    })
}

fn rx_queue_is_empty(mode: WifiDeviceMode) -> bool {
    queue_for(mode).with(|queue| queue.is_empty())
}

pub(crate) fn tx_can_send() -> bool {
    WIFI_TX_INFLIGHT.load(Ordering::SeqCst) < TX_QUEUE_SIZE.load(Ordering::Relaxed)
}

pub(crate) fn increase_tx_inflight() {
    WIFI_TX_INFLIGHT.fetch_add(1, Ordering::SeqCst);
}

fn tx_token_ready() -> bool {
    if !tx_can_send() {
        crate::preempt::yield_task();
    }

    tx_can_send()
}

fn rx_token_ready(mode: WifiDeviceMode, can_send: bool) -> bool {
    let is_empty = rx_queue_is_empty(mode);
    if is_empty || !can_send {
        crate::preempt::yield_task();
    }

    if is_empty {
        !rx_queue_is_empty(mode)
    } else {
        true
    }
}

fn pop_rx_packet(mode: WifiDeviceMode) -> EspWifiPacketBuffer {
    queue_for(mode).with(|queue| {
        queue.pop_front()
            .expect("unreachable: receive path checked queue state before pop")
    })
}

pub(crate) fn tx_token(mode: WifiDeviceMode) -> Option<WifiTxToken> {
    if tx_token_ready() {
        Some(WifiTxToken { mode })
    } else {
        None
    }
}

pub(crate) fn rx_token(mode: WifiDeviceMode, can_send: bool) -> Option<(WifiRxToken, WifiTxToken)> {
    if rx_token_ready(mode, can_send) {
        tx_token(mode).map(|tx| (WifiRxToken { mode }, tx))
    } else {
        None
    }
}

pub(crate) fn register_receive_waker(mode: WifiDeviceMode, cx: &mut Context<'_>) {
    match mode {
        WifiDeviceMode::Sta => embassy::STA_RECEIVE_WAKER.register(cx.waker()),
        WifiDeviceMode::Ap => embassy::AP_RECEIVE_WAKER.register(cx.waker()),
    }
}

pub(crate) fn consume_rx_token<R, F>(mode: WifiDeviceMode, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    let mut data = pop_rx_packet(mode);
    let buffer = data.as_slice_mut();
    dump_packet_info(buffer);
    f(buffer)
}

pub(crate) fn consume_tx_token<R, F>(mode: WifiDeviceMode, len: usize, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    increase_tx_inflight();

    static mut BUFFER: [u8; super::super::MTU] = [0u8; super::super::MTU];
    let buffer = unsafe { &mut BUFFER[..len] };
    let res = f(buffer);
    esp_wifi_send_data(mode.interface(), buffer);
    res
}

pub(crate) unsafe extern "C" fn recv_cb_sta(
    buffer: *mut c_void,
    len: u16,
    eb: *mut c_void,
) -> i32 {
    WIFI_RX_CB_STA_COUNT.fetch_add(1, Ordering::Relaxed);
    let packet = EspWifiPacketBuffer { buffer, len, eb };
    match enqueue_rx_packet(
        WifiDeviceMode::Sta,
        packet,
        RX_QUEUE_SIZE.load(Ordering::Relaxed),
    ) {
        Ok(()) => {
            embassy::STA_RECEIVE_WAKER.wake();
            include::ESP_OK as i32
        }
        Err(_) => include::ESP_ERR_NO_MEM as i32,
    }
}

pub(crate) unsafe extern "C" fn recv_cb_ap(
    buffer: *mut c_void,
    len: u16,
    eb: *mut c_void,
) -> i32 {
    WIFI_RX_CB_AP_COUNT.fetch_add(1, Ordering::Relaxed);
    let packet = EspWifiPacketBuffer { buffer, len, eb };
    match enqueue_rx_packet(
        WifiDeviceMode::Ap,
        packet,
        RX_QUEUE_SIZE.load(Ordering::Relaxed),
    ) {
        Ok(()) => {
            embassy::AP_RECEIVE_WAKER.wake();
            include::ESP_OK as i32
        }
        Err(_) => include::ESP_ERR_NO_MEM as i32,
    }
}

#[cfg(all(feature = "sniffer", feature = "unstable"))]
fn sniffer_get() -> Option<fn(PromiscuousPkt<'_>)> {
    SNIFFER_CB.with(|callback| *callback)
}

#[cfg(all(feature = "sniffer", feature = "unstable"))]
pub(crate) fn sniffer_set(cb: fn(PromiscuousPkt<'_>)) {
    SNIFFER_CB.with(|callback| *callback = Some(cb));
}

#[cfg(all(feature = "sniffer", feature = "unstable"))]
pub(crate) unsafe extern "C" fn promiscuous_rx_cb(
    buf: *mut core::ffi::c_void,
    frame_type: u32,
) {
    super::super::WIFI_PROMISC_RX_CB_COUNT.fetch_add(1, Ordering::Relaxed);
    if let Some(sniffer_callback) = sniffer_get() {
        let promiscuous_pkt = PromiscuousPkt::from_raw(buf.cast_const().cast(), frame_type);
        sniffer_callback(promiscuous_pkt);
    }
}
