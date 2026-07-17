use alloc::collections::vec_deque::VecDeque;
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{binary::include, compat::legacy_runtime_policy::backend_legacy_port_enabled};

use super::{
    WIFI_TX_INFLIGHT,
    WifiDeviceMode,
    RX_QUEUE_SIZE,
    TX_QUEUE_SIZE,
    WIFI_RX_CB_AP_COUNT,
    WIFI_RX_CB_STA_COUNT,
};
#[cfg(all(feature = "sniffer", feature = "unstable"))]
use super::{PromiscuousPkt, WIFI_PROMISC_RX_CB_COUNT};

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

/// Take care not to drop this while in a critical section.
///
/// Dropping an `EspWifiPacketBuffer` will call
/// `esp_wifi_internal_free_rx_buffer`, which can lock an internal mutex and
/// trigger a context switch.
#[derive(Debug)]
pub(crate) struct EspWifiPacketBuffer {
    pub(crate) buffer: *mut crate::binary::c_types::c_void,
    pub(crate) len: u16,
    pub(crate) eb: *mut crate::binary::c_types::c_void,
}

unsafe impl Send for EspWifiPacketBuffer {}

impl Drop for EspWifiPacketBuffer {
    fn drop(&mut self) {
        unsafe { crate::binary::include::esp_wifi_internal_free_rx_buffer(self.eb) };
    }
}

impl EspWifiPacketBuffer {
    pub(crate) fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.buffer as *mut u8, self.len as usize) }
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

pub(crate) fn enqueue_rx_packet(
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

pub(crate) fn rx_queue_is_empty(mode: WifiDeviceMode) -> bool {
    queue_for(mode).with(|queue| queue.is_empty())
}

pub(crate) fn tx_can_send() -> bool {
    WIFI_TX_INFLIGHT.load(Ordering::SeqCst) < TX_QUEUE_SIZE.load(Ordering::Relaxed)
}

pub(crate) fn increase_tx_inflight() {
    WIFI_TX_INFLIGHT.fetch_add(1, Ordering::SeqCst);
}

pub(crate) fn tx_token_ready() -> bool {
    if !tx_can_send() {
        crate::preempt::yield_task();
    }

    tx_can_send()
}

pub(crate) fn rx_token_ready(mode: WifiDeviceMode, can_send: bool) -> bool {
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

pub(crate) fn pop_rx_packet(mode: WifiDeviceMode) -> EspWifiPacketBuffer {
    queue_for(mode).with(|queue| {
        queue.pop_front()
            .expect("unreachable: receive path checked queue state before pop")
    })
}

pub(crate) unsafe extern "C" fn recv_cb_sta(
    buffer: *mut crate::binary::c_types::c_void,
    len: u16,
    eb: *mut crate::binary::c_types::c_void,
) -> i32 {
    WIFI_RX_CB_STA_COUNT.fetch_add(1, Ordering::Relaxed);
    let packet = EspWifiPacketBuffer { buffer, len, eb };
    match enqueue_rx_packet(
        WifiDeviceMode::Sta,
        packet,
        RX_QUEUE_SIZE.load(Ordering::Relaxed),
    ) {
        Ok(()) => {
            crate::wifi::embassy::STA_RECEIVE_WAKER.wake();
            include::ESP_OK as i32
        }
        Err(_) => include::ESP_ERR_NO_MEM as i32,
    }
}

pub(crate) unsafe extern "C" fn recv_cb_ap(
    buffer: *mut crate::binary::c_types::c_void,
    len: u16,
    eb: *mut crate::binary::c_types::c_void,
) -> i32 {
    WIFI_RX_CB_AP_COUNT.fetch_add(1, Ordering::Relaxed);
    let packet = EspWifiPacketBuffer { buffer, len, eb };
    match enqueue_rx_packet(
        WifiDeviceMode::Ap,
        packet,
        RX_QUEUE_SIZE.load(Ordering::Relaxed),
    ) {
        Ok(()) => {
            crate::wifi::embassy::AP_RECEIVE_WAKER.wake();
            include::ESP_OK as i32
        }
        Err(_) => include::ESP_ERR_NO_MEM as i32,
    }
}

#[cfg(all(feature = "sniffer", feature = "unstable"))]
pub(crate) fn sniffer_get() -> Option<fn(PromiscuousPkt<'_>)> {
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
    WIFI_PROMISC_RX_CB_COUNT.fetch_add(1, Ordering::Relaxed);
    if let Some(sniffer_callback) = sniffer_get() {
        let promiscuous_pkt = PromiscuousPkt::from_raw(buf as *const _, frame_type);
        sniffer_callback(promiscuous_pkt);
    }
}
