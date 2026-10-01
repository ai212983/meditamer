//! One-shot BLE central connection, GATT discovery, and optional HID capture.
//!
//! The runtime finds a device by advertised name, connects, optionally pairs,
//! discovers services and characteristics, reads the first readable value, and
//! disconnects. All collections and capture buffers have fixed capacities.

use embassy_time::Duration;
use esp_radio::ble::controller::BleConnector;
use heapless::Vec;
use trouble_host::prelude::{DefaultPacketPool, ExternalController, HostResources, Uuid};

mod discovery;
mod hid_capture;
mod protocol;
mod runtime;

pub use runtime::{run, run_with_resources};

pub(super) const CONNECTIONS_MAX: usize = 1;
pub(super) const L2CAP_CHANNELS_MAX: usize = 2;
pub(super) const MAX_DISCOVERED_SERVICES: usize = 8;
pub(super) const MAX_CHARACTERISTICS_PER_SERVICE: usize = crate::capacity::GATT_CHARACTERISTICS_MAX;
pub(super) const READ_BUFFER_MAX: usize = 32;
pub(super) const HID_SERVICE_UUID: u16 = 0x1812;
pub(super) const HID_REPORT_MAP_UUID: u16 = 0x2a4b;
pub(super) const HID_REPORT_UUID: u16 = 0x2a4d;
pub(super) const HID_REPORT_REFERENCE_UUID: u16 = 0x2908;

pub(super) type Controller<'d> = ExternalController<BleConnector<'d>, 1>;
pub type Resources = HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX>;

/// Inputs for one live central connection.
#[derive(Clone, Copy)]
pub struct LiveConnectOptions<'a> {
    /// Substring matched against advertised local names.
    pub target_name: &'a str,
    /// Maximum time spent looking for the target.
    pub scan_timeout: Duration,
    /// Deadline for scan startup and each connection operation after scanning.
    pub connect_timeout: Duration,
    /// Caller-supplied monotonic microsecond clock.
    pub now_ticks: fn() -> u64,
    /// Called once after host initialization and scan enable complete.
    /// Not called on startup failure; callers must also handle runner return.
    pub startup_complete: Option<fn()>,
    /// Allow security negotiation and persist a completed bond in memory.
    pub request_pairing: bool,
    /// Initiate pairing before service discovery instead of on an ATT error.
    pub pair_before_discovery: bool,
    /// Optional absolute-duration window for retained HID notifications.
    pub hid_capture_duration: Option<Duration>,
    /// Optional synchronous consumer for HID input reports.
    pub hid_input_sink: Option<&'static dyn HidInputSink>,
    /// Optional cooperative cancellation predicate. It is sampled between
    /// operations and while waiting for HID notifications; the normal
    /// controller shutdown and quiescence checks run before the call returns.
    pub stop_requested: Option<fn() -> bool>,
}

/// How far [`run`] got before stopping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveConnectOutcome {
    /// Every attempted step (through disconnect) completed.
    Completed,
    /// `BleConnector::new` returned an initialization error.
    ControllerInit,
    /// Starting the scan failed or exceeded the connection deadline.
    ScanStartFailed,
    /// The name scan window closed without finding the target device.
    DeviceNotFound,
    /// `Central::connect` failed or timed out.
    ConnectFailed,
    /// `GattClient::new` (MTU exchange) failed.
    GattClientInitFailed,
    /// Requesting security failed before an in-progress pairing existed.
    SecurityRequestFailed,
    /// The peer explicitly rejected an in-progress pairing.
    PairingFailed,
    /// The peer did not answer an in-progress pairing before its deadline.
    PairingTimedOut,
    /// The peer terminated the link while pairing was pending.
    DisconnectedDuringPairing,
    /// Primary-service discovery errored or timed out.
    ServiceDiscoveryFailed,
    /// The peer terminated the link during primary-service discovery.
    DisconnectedDuringServiceDiscovery,
    /// The host runner or GATT dispatcher exited unexpectedly.
    HostExited,
    /// The peer terminated the link during optional HID capture.
    DisconnectedDuringHidCapture,
    /// The caller requested a cooperative stop; controller teardown follows.
    Cancelled,
}

/// Machine-readable cause captured when primary-service discovery fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceDiscoveryFailure {
    Timeout,
    Disconnected(u8),
    Controller,
    Att(u8),
    InsufficientSpace,
    InvalidValue,
    UnexpectedGattResponse,
    InvalidUuidLength(usize),
    OtherHost,
}

/// State sampled at the instant primary-service discovery failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceDiscoveryDiagnostic {
    pub failure: ServiceDiscoveryFailure,
    pub elapsed_us: u64,
    pub att_mtu: u16,
    pub link_connected: bool,
    /// Raw HCI status from a disconnect event already queued at failure.
    pub disconnect_reason: Option<u8>,
}

/// A discovered service or characteristic UUID in its declared width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveredUuid {
    Uuid16(u16),
    Uuid32(u32),
    Uuid128([u8; 16]),
}

impl From<Uuid> for DiscoveredUuid {
    fn from(uuid: Uuid) -> Self {
        match uuid {
            Uuid::Uuid16(bytes) => Self::Uuid16(u16::from_le_bytes(bytes)),
            Uuid::Uuid32(bytes) => Self::Uuid32(u32::from_le_bytes(bytes)),
            Uuid::Uuid128(bytes) => Self::Uuid128(bytes),
        }
    }
}

/// One discovered service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscoveredService {
    pub uuid: DiscoveredUuid,
    pub start_handle: u16,
    pub end_handle: u16,
}

/// One discovered characteristic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscoveredCharacteristic {
    pub uuid: DiscoveredUuid,
    pub handle: u16,
    pub readable: bool,
    pub writable: bool,
    pub notifiable: bool,
}

/// HID Report Reference descriptor plus the value and CCCD handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HidReportReference {
    pub characteristic_handle: u16,
    pub cccd_handle: Option<u16>,
    pub report_id: u8,
    /// HID Report Reference type: 1=input, 2=output, 3=feature.
    pub report_type: u8,
}

/// One raw HID notification retained by the bounded capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedHidReport {
    pub characteristic_handle: u16,
    pub report_id: u8,
    pub captured_at_us: u64,
    pub bytes: Vec<u8, { crate::capacity::HID_REPORT_MAX_BYTES }>,
}

/// Synchronous consumer for a live HID input stream.
pub trait HidInputSink: Sync {
    fn input_ready(&self, now_us: u64);
    fn report(&self, report_id: u8, now_us: u64, bytes: &[u8]);
    fn input_closed(&self, now_us: u64);
}

/// The outcome of [`run`] plus progress captured before it stopped.
pub struct LiveConnectReport {
    pub outcome: LiveConnectOutcome,
    pub address: Option<[u8; 6]>,
    pub connection_state: crate::gatt::ConnectionState,
    /// Check `outcome` before interpreting an empty collection as a peer
    /// that genuinely exposed no primary services.
    pub services: Vec<DiscoveredService, MAX_DISCOVERED_SERVICES>,
    pub characteristics: Vec<
        DiscoveredCharacteristic,
        { MAX_DISCOVERED_SERVICES * MAX_CHARACTERISTICS_PER_SERVICE },
    >,
    pub read_handle: Option<u16>,
    pub read_bytes: Vec<u8, READ_BUFFER_MAX>,
    pub hid_report_map: Vec<u8, { crate::capacity::HID_DESCRIPTOR_MAX_BYTES }>,
    pub hid_reports: Vec<HidReportReference, { crate::capacity::HID_REPORTS_MAX }>,
    pub hid_subscriptions: u8,
    pub hid_notifications:
        Vec<CapturedHidReport, { crate::capacity::HID_NOTIFICATION_CAPTURE_MAX }>,
    pub security_level: Option<crate::bond::SecurityLevel>,
    pub bonded: bool,
    pub bond_table_full: bool,
    pub pairing_elapsed_us: Option<u64>,
    pub pairing_disconnect_reason: Option<u8>,
    pub service_discovery_diagnostic: Option<ServiceDiscoveryDiagnostic>,
}

impl LiveConnectReport {
    /// Empty caller-owned storage for one live-connect attempt.
    pub const fn new() -> Self {
        Self {
            outcome: LiveConnectOutcome::ControllerInit,
            address: None,
            connection_state: crate::gatt::ConnectionState::Idle,
            services: Vec::new(),
            characteristics: Vec::new(),
            read_handle: None,
            read_bytes: Vec::new(),
            hid_report_map: Vec::new(),
            hid_reports: Vec::new(),
            hid_subscriptions: 0,
            hid_notifications: Vec::new(),
            security_level: None,
            bonded: false,
            bond_table_full: false,
            pairing_elapsed_us: None,
            pairing_disconnect_reason: None,
            service_discovery_diagnostic: None,
        }
    }

    fn reset(&mut self) {
        self.outcome = LiveConnectOutcome::ControllerInit;
        self.address = None;
        self.connection_state = crate::gatt::ConnectionState::Idle;
        self.services.clear();
        self.characteristics.clear();
        self.read_handle = None;
        self.read_bytes.clear();
        self.hid_report_map.clear();
        self.hid_reports.clear();
        self.hid_subscriptions = 0;
        self.hid_notifications.clear();
        self.security_level = None;
        self.bonded = false;
        self.bond_table_full = false;
        self.pairing_elapsed_us = None;
        self.pairing_disconnect_reason = None;
        self.service_discovery_diagnostic = None;
    }
}

impl Default for LiveConnectReport {
    fn default() -> Self {
        Self::new()
    }
}
