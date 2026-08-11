//! Bluetooth Low Energy HCI interface
//!
//! The usage of BLE is currently incompatible with the usage of IEEE 802.15.4.

#[cfg(bt_controller = "btdm")]
pub(crate) mod btdm;

#[cfg(bt_controller = "npl")]
pub(crate) mod npl;

#[cfg(bt_controller = "btdm")]
mod tx_cancellation;

#[cfg(bt_controller = "npl")]
use core::mem::MaybeUninit;

pub(crate) use ble::{ble_deinit, ble_init, send_hci};
#[cfg(bt_controller = "btdm")]
pub use btdm::{begin_hci_callback_shutdown, hci_callback_stats, wait_for_hci_callback_quiescence};
use docsplay::Display;
#[cfg(bt_controller = "btdm")]
use embassy_sync::mutex::Mutex;
use esp_sync::NonReentrantMutex;
#[cfg(bt_controller = "btdm")]
use esp_sync::RawMutex;
use heapless::{Deque, Vec};
use portable_atomic::{AtomicBool, AtomicU32, Ordering};

pub(crate) const HCI_PACKET_CAPACITY: usize = 259;
pub(crate) const RX_QUEUE_CAPACITY: usize = 4;

static RX_QUEUE_OVERFLOW_COUNT: AtomicU32 = AtomicU32::new(0);
static RX_OVERSIZE_COUNT: AtomicU32 = AtomicU32::new(0);
static RX_QUEUE_HIGH_WATER: AtomicU32 = AtomicU32::new(0);
static TX_REJECTED_COUNT: AtomicU32 = AtomicU32::new(0);
static TX_TIMEOUT_COUNT: AtomicU32 = AtomicU32::new(0);
static TRANSPORT_FAULTED: AtomicBool = AtomicBool::new(false);

/// An error that is returned when the configuration is invalid.
#[derive(Display, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub struct InvalidConfigError;

impl core::error::Error for InvalidConfigError {}

// Expose chip-specific configuration types
pub use ble::ble_os_adapter_chip_specific::*;

#[cfg(bt_controller = "btdm")]
use self::btdm as ble;
#[cfg(bt_controller = "npl")]
use self::npl as ble;

unstable_module! {
    pub mod controller;
}

pub(crate) unsafe extern "C" fn malloc(size: u32) -> *mut crate::sys::c_types::c_void {
    unsafe { crate::compat::malloc::malloc(size as usize).cast() }
}

#[cfg(any(esp32, esp32c3, esp32s3))]
pub(crate) unsafe extern "C" fn malloc_internal(size: u32) -> *mut crate::sys::c_types::c_void {
    unsafe { crate::compat::malloc::malloc_internal(size as usize).cast() }
}

pub(crate) unsafe extern "C" fn free(ptr: *mut crate::sys::c_types::c_void) {
    unsafe { crate::compat::malloc::free(ptr.cast()) }
}

struct BleState {
    pub rx_queue: Deque<ReceivedPacket, RX_QUEUE_CAPACITY>,
    pub hci_read_data: Vec<u8, HCI_PACKET_CAPACITY>,
}

static BT_STATE: NonReentrantMutex<BleState> = NonReentrantMutex::new(BleState {
    rx_queue: Deque::new(),
    hci_read_data: Vec::new(),
});

#[cfg(bt_controller = "btdm")]
static HCI_OUT_COLLECTOR: Mutex<RawMutex, HciOutCollector> = Mutex::new(HciOutCollector::new());
#[cfg(bt_controller = "npl")]
static mut HCI_OUT_COLLECTOR: MaybeUninit<HciOutCollector> = MaybeUninit::uninit();

#[derive(PartialEq, Debug)]
enum HciOutType {
    Unknown,
    Acl,
    Command,
}

struct HciOutCollector {
    data: [u8; HCI_PACKET_CAPACITY],
    index: usize,
    ready: bool,
    kind: HciOutType,
}

impl HciOutCollector {
    const fn new() -> HciOutCollector {
        HciOutCollector {
            data: [0u8; HCI_PACKET_CAPACITY],
            index: 0,
            ready: false,
            kind: HciOutType::Unknown,
        }
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn push(&mut self, data: &[u8]) -> Result<(), HciTransportError> {
        let Some(end) = self.index.checked_add(data.len()) else {
            return Err(HciTransportError::TxPacketTooLarge);
        };
        if end > self.data.len() {
            return Err(HciTransportError::TxPacketTooLarge);
        }

        self.data[self.index..end].copy_from_slice(data);
        self.index = end;
        if self.index == 0 {
            return Ok(());
        }

        if self.kind == HciOutType::Unknown {
            self.kind = match self.data[0] {
                1 => HciOutType::Command,
                2 => HciOutType::Acl,
                _ => HciOutType::Unknown,
            };
        }

        if !self.ready {
            if self.kind == HciOutType::Command && self.index >= 4 {
                if self.index == self.data[3] as usize + 4 {
                    self.ready = true;
                }
            } else if self.kind == HciOutType::Acl
                && self.index >= 5
                && self.index == (self.data[3] as usize) + ((self.data[4] as usize) << 8) + 5
            {
                self.ready = true;
            }
        }

        Ok(())
    }

    fn reset(&mut self) {
        self.index = 0;
        self.ready = false;
        self.kind = HciOutType::Unknown;
    }

    fn packet(&self) -> &[u8] {
        &self.data[0..self.index]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Represents a received BLE packet.
#[instability::unstable]
pub struct ReceivedPacket {
    /// The data of the received packet.
    pub data: Vec<u8, HCI_PACKET_CAPACITY>,
}

/// Bounded HCI transport failures.
#[derive(Display, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[instability::unstable]
pub enum HciTransportError {
    /// The host supplied an HCI packet larger than the fixed transport buffer.
    TxPacketTooLarge,
    /// The controller did not become available or acknowledge transmission before the deadline.
    TxTimeout,
    /// A prior timeout fault remains latched until the controller is reinitialized.
    Faulted,
}

/// Monotonic counters for observable bounded-transport fault handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[instability::unstable]
pub struct HciTransportStats {
    /// Packets dropped because the fixed receive queue was full.
    pub rx_queue_overflow: u32,
    /// Packets dropped because they exceeded the fixed packet capacity.
    pub rx_oversize: u32,
    /// Highest observed number of queued receive packets.
    pub rx_queue_high_water: u32,
    /// Host packets rejected because they exceeded the fixed transmit capacity.
    pub tx_rejected: u32,
    /// Transmissions aborted after a controller availability or completion deadline.
    pub tx_timeout: u32,
    /// Whether a timeout fault is latched and transmit is disabled until reinitialization.
    pub faulted: bool,
}

/// Snapshot of controller-to-host callback admission and quiescence state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[instability::unstable]
pub struct HciCallbackStats {
    /// Whether callback ingress is currently admitted for the active epoch.
    pub admission_open: bool,
    /// Number of callbacks accepted in the active epoch.
    pub accepted: u32,
    /// Number of callbacks rejected after ingress revocation.
    pub rejected: u32,
    /// Callbacks currently executing, including rejected callback stubs.
    pub in_flight: u32,
    /// Highest observed number of simultaneously executing callbacks.
    pub high_water: u32,
}

/// Returns a snapshot of the bounded HCI transport counters.
#[instability::unstable]
pub fn hci_transport_stats() -> HciTransportStats {
    HciTransportStats {
        rx_queue_overflow: RX_QUEUE_OVERFLOW_COUNT.load(Ordering::Relaxed),
        rx_oversize: RX_OVERSIZE_COUNT.load(Ordering::Relaxed),
        rx_queue_high_water: RX_QUEUE_HIGH_WATER.load(Ordering::Relaxed),
        tx_rejected: TX_REJECTED_COUNT.load(Ordering::Relaxed),
        tx_timeout: TX_TIMEOUT_COUNT.load(Ordering::Relaxed),
        faulted: TRANSPORT_FAULTED.load(Ordering::Relaxed),
    }
}

pub(crate) fn record_rx_queue_overflow() {
    RX_QUEUE_OVERFLOW_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_rx_oversize() {
    RX_OVERSIZE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_rx_queue_depth(depth: usize) {
    let depth = depth as u32;
    let mut observed = RX_QUEUE_HIGH_WATER.load(Ordering::Relaxed);
    while depth > observed {
        match RX_QUEUE_HIGH_WATER.compare_exchange_weak(
            observed,
            depth,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(actual) => observed = actual,
        }
    }
}

pub(crate) fn record_tx_rejected() {
    TX_REJECTED_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_tx_timeout() {
    TX_TIMEOUT_COUNT.fetch_add(1, Ordering::Relaxed);
    latch_transport_fault();
}

pub(crate) fn latch_transport_fault() {
    TRANSPORT_FAULTED.store(true, Ordering::Release);
}

pub(crate) fn transport_faulted() -> bool {
    TRANSPORT_FAULTED.load(Ordering::Acquire)
}

pub(crate) fn reset_transport_state() {
    BT_STATE.with(|state| {
        state.rx_queue.clear();
        state.hci_read_data.clear();
    });
    RX_QUEUE_OVERFLOW_COUNT.store(0, Ordering::Relaxed);
    RX_OVERSIZE_COUNT.store(0, Ordering::Relaxed);
    RX_QUEUE_HIGH_WATER.store(0, Ordering::Relaxed);
    TX_REJECTED_COUNT.store(0, Ordering::Relaxed);
    TX_TIMEOUT_COUNT.store(0, Ordering::Relaxed);
    TRANSPORT_FAULTED.store(false, Ordering::Relaxed);
}

#[cfg(feature = "defmt")]
impl defmt::Format for ReceivedPacket {
    fn format(&self, fmt: defmt::Formatter<'_>) {
        defmt::write!(fmt, "ReceivedPacket {}", &self.data[..])
    }
}

/// Checks if there is any HCI data available to read.
#[instability::unstable]
pub fn have_hci_read_data() -> bool {
    BT_STATE.with(|state| !state.rx_queue.is_empty() || !state.hci_read_data.is_empty())
}

pub(crate) fn read_next(data: &mut [u8]) -> usize {
    if let Some(packet) = BT_STATE.with(|state| state.rx_queue.pop_front()) {
        data[..packet.data.len()].copy_from_slice(&packet.data[..packet.data.len()]);
        packet.data.len()
    } else {
        0
    }
}

/// Reads the next HCI packet from the BLE controller.
#[instability::unstable]
pub fn read_hci(data: &mut [u8]) -> usize {
    BT_STATE.with(|state| {
        if state.hci_read_data.is_empty() {
            if let Some(packet) = state.rx_queue.pop_front() {
                if state.hci_read_data.extend_from_slice(&packet.data).is_err() {
                    record_rx_oversize();
                }
            }
        }

        let l = usize::min(state.hci_read_data.len(), data.len());
        data[..l].copy_from_slice(&state.hci_read_data[..l]);
        state.hci_read_data.drain(..l);
        l
    })
}

fn dump_packet_info(_buffer: &[u8]) {
    #[cfg(dump_packets)]
    info!("@HCIFRAME {:?}", _buffer);
}

macro_rules! validate_range {
    ($this:ident, $field:ident, $min:expr, $max:expr) => {
        if !($min..=$max).contains(&$this.$field) {
            error!(
                "{} must be between {} and {}, current value is {}",
                stringify!($field),
                $min,
                $max,
                $this.$field
            );
            return Err(InvalidConfigError);
        }
    };
}
pub(crate) use validate_range;
