//! Non-default BLE foundation probe.
//!
//! Exposes bounded diagnostic BLE service and probe lifecycle ownership.

// trouble-host derive expansions trigger this lint at unchanged field-type
// spans; keep the exception inside the non-default probe module.
#![allow(clippy::needless_borrows_for_generic_args)]

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

mod runtime;
mod service;

pub use runtime::Phase1Service;

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Duration;
use esp_radio::ble::{controller::BleConnector, hci_callback_stats, hci_transport_stats};
use trouble_host::prelude::*;

ble::impl_bounded_packet_pool!(
    Phase1PacketPool,
    Phase1Packet,
    mtu = PACKET_MTU,
    capacity = PACKET_COUNT
);

use ble::reusable_slot::ReusableSlot;

// ADR-0011's diagnostic v1 GATT server has stable wire UUIDs.
// `#[gatt_server]`/`#[gatt_service]` need `trouble-host`'s
// `derive`/`peripheral` features, which only this crate's own `trouble-host`
// dependency enables (`platform/connectivity/ble`'s does not, by design -- see that
// crate's Cargo.toml), so the server itself lives here, not in
// `platform/connectivity/ble`; it is built from `ble::diagnostic`'s wire schema and
// `ble::address`'s address generation, not a copy of either.
// Trouble 0.8 and the product both use embassy-sync 0.8, so the generated
// attribute server and this module's signals now share one RawMutex trait.
#[gatt_server(connections_max = CONNECTIONS_MAX, mutex_type = embassy_sync::blocking_mutex::raw::NoopRawMutex, packet_type = Phase1PacketPool, attribute_table_size = 32)]
struct DiagnosticServer {
    diagnostic: DiagnosticService,
}

#[gatt_service(uuid = "BE836D30-5EA2-4E16-B476-4D32E4F7CBCE")]
struct DiagnosticService {
    /// ADR-0011 Build Info: read-only, fixed 16 bytes (`ble::diagnostic::BuildInfo`).
    #[characteristic(uuid = "BE836D30-5EA2-4E16-B476-4D32E4F7CBCF", read, value = [0u8; ble::capacity::BUILD_INFO_BYTES])]
    build_info: [u8; ble::capacity::BUILD_INFO_BYTES],
    /// ADR-0011 Echo: read/write/notify, 1-32 bytes (`ble::diagnostic::EchoRateLimiter`
    /// admits writes; this server echoes the written payload back via notify).
    #[characteristic(uuid = "BE836D30-5EA2-4E16-B476-4D32E4F7CBD0", read, write, notify)]
    echo: heapless::Vec<u8, { ble::capacity::ECHO_PAYLOAD_MAX_BYTES }>,
    /// ADR-0011 Lifecycle Status: read/notify, fixed 8 bytes
    /// (`ble::diagnostic::LifecycleStatus`/`LifecycleStatusPublisher`).
    #[characteristic(uuid = "BE836D30-5EA2-4E16-B476-4D32E4F7CBD1", read, notify, value = [0u8; ble::capacity::LIFECYCLE_STATUS_BYTES])]
    lifecycle_status: [u8; ble::capacity::LIFECYCLE_STATUS_BYTES],
}

/// The previous epoch's advertising address, so [`ble::address::generate_epoch_address`]
/// can reject an immediate repeat (ADR-0011). `None` before the first
/// successful generation.
static PHASE1S_PREVIOUS_ADDRESS: critical_section::Mutex<
    core::cell::Cell<Option<ble::address::EpochAddress>>,
> = critical_section::Mutex::new(core::cell::Cell::new(None));

/// Draws entropy from the chip's hardware RNG and generates this epoch's
/// advertising address, retrying a bounded number of times against
/// [`ble::address::generate_epoch_address`]'s (astronomically unlikely)
/// rejection cases before giving up.
fn generate_and_record_epoch_address() -> Result<Address, ProbeFailure> {
    let previous = critical_section::with(|cs| PHASE1S_PREVIOUS_ADDRESS.borrow(cs).get());
    let rng = esp_hal::rng::Rng::new();
    let mut entropy = [0u8; 6];
    for _ in 0..8 {
        rng.read(&mut entropy);
        if let Ok(address) = ble::address::generate_epoch_address(entropy, previous) {
            critical_section::with(|cs| {
                PHASE1S_PREVIOUS_ADDRESS.borrow(cs).set(Some(address));
            });
            return Ok(Address::random(address.bytes()));
        }
    }
    Err(ProbeFailure::AddressGeneration)
}

const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 2;
const PACKET_MTU: usize = 64;
const PACKET_COUNT: usize = 4;
const PHASE1S_CYCLES: u8 = 1;
const REQUIRED_PREINIT_FREE: usize = 20_496;
const REQUIRED_PREINIT_BLOCK: usize = 4_112;
const INTERNAL_RESERVE: usize = 16_384;
const HOST_INIT_TIMEOUT: Duration = Duration::from_secs(2);
// ADR-0011 bounds callback teardown acknowledgement to two seconds.
const CALLBACK_QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(2);
const CALLBACK_QUIET_DWELL: Duration = Duration::from_millis(20);
/// One tick is one microsecond, matching `esp_hal::time::Instant`'s own
/// resolution; every `ble::deadline`/`ble::diagnostic::EchoRateLimiter` call
/// in this module uses this same unit.
const ONE_SECOND_TICKS: u64 = 1_000_000;
/// ADR-0011's advertising, connection, idle, and window deadlines.
const DEADLINE_CONFIG: ble::deadline::DeadlineConfig =
    ble::deadline::DeadlineConfig::adr_0011_defaults(ONE_SECOND_TICKS);
const BUILD_ID: &str = match option_env!("MEDITAMER_FIRMWARE_BUILD_ID") {
    Some(value) => value,
    None => "unlabeled",
};

type BleController<'d> = ExternalController<BleConnector<'d>, 1>;
type Resources = HostResources<Phase1PacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX>;

static PHASE1S_REQUEST: Signal<CriticalSectionRawMutex, ProbeRequest> = Signal::new();
static PHASE1S_CLOSE_REQUEST: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static PHASE1D_STATE: AtomicU8 = AtomicU8::new(ProbeState::Idle as u8);
static PHASE1D_CYCLE: AtomicU8 = AtomicU8::new(0);
static PHASE1D_FAILURE: AtomicU8 = AtomicU8::new(ProbeFailure::None as u8);
static PHASE1S_BOOT: AtomicU32 = AtomicU32::new(0);
static PHASE1S_EPOCH: AtomicU32 = AtomicU32::new(0);
static PHASE1S_BEFORE_FREE: AtomicU32 = AtomicU32::new(0);
static PHASE1S_CONTROLLER_FREE: AtomicU32 = AtomicU32::new(0);
static PHASE1S_ACTIVE_FREE: AtomicU32 = AtomicU32::new(0);
static PHASE1S_AFTER_FREE: AtomicU32 = AtomicU32::new(0);
static PHASE1S_CALLBACKS_REJECTED: AtomicU32 = AtomicU32::new(0);
static PHASE1S_RX_QUEUE_OVERFLOW: AtomicU32 = AtomicU32::new(0);
static PHASE1S_RX_OVERSIZE: AtomicU32 = AtomicU32::new(0);
static PHASE1S_TX_REJECTED: AtomicU32 = AtomicU32::new(0);
static PHASE1S_TX_TIMEOUT: AtomicU32 = AtomicU32::new(0);
static PHASE1S_QUEUE_TASK_CANCELLED: AtomicU32 = AtomicU32::new(0);
static HOST_RESOURCES: ReusableSlot<Resources> = ReusableSlot::new();
// Built once at task startup, not per cycle (unlike HOST_RESOURCES/HOST_STACK
// above): `#[gatt_service]`'s generated storage for any characteristic value
// over 8 bytes (Echo, Build Info) is its own plain, one-shot
// `static_cell::StaticCell`, not a `ReusableSlot` -- calling
// `DiagnosticServer::new_with_config` a second time re-`.init()`s that same
// static and panics ("StaticCell is already full"). Confirmed on real
// hardware (E-0016): the second diagnostic window a device ever serves
// crashed here. The attribute table itself has no per-connection state that
// needs resetting between cycles (unlike the connection/HCI/callback layers,
// which genuinely do), so building it once and reusing it is correct, not
// just a workaround.
static DIAGNOSTIC_SERVER: static_cell::StaticCell<DiagnosticServer<'static>> =
    static_cell::StaticCell::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProbeRequest {
    boot_generation: u32,
    epoch: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum ProbeState {
    Idle,
    Queued,
    Running,
    Completed,
    Failed,
    OwnershipUnknown,
}

impl ProbeState {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Queued,
            2 => Self::Running,
            3 => Self::Completed,
            4 => Self::Failed,
            5 => Self::OwnershipUnknown,
            _ => Self::Idle,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::OwnershipUnknown => "ownership_unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum ProbeFailure {
    None,
    ExclusiveLease,
    UpdateReserved,
    ResourceFloor,
    ControllerInit,
    HostExited,
    CallbackQuiescence,
    LateCallback,
    PacketLeak,
    QueueLifecycle,
    TransportFault,
    PacketExhausted,
    HostInit,
    AddressGeneration,
    GattServerInit,
}

impl ProbeFailure {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::ExclusiveLease,
            2 => Self::UpdateReserved,
            3 => Self::ResourceFloor,
            4 => Self::ControllerInit,
            5 => Self::HostExited,
            6 => Self::CallbackQuiescence,
            7 => Self::LateCallback,
            8 => Self::PacketLeak,
            9 => Self::QueueLifecycle,
            10 => Self::TransportFault,
            11 => Self::PacketExhausted,
            12 => Self::HostInit,
            13 => Self::AddressGeneration,
            14 => Self::GattServerInit,
            _ => Self::None,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ExclusiveLease => "exclusive_lease",
            Self::UpdateReserved => "update_reserved",
            Self::ResourceFloor => "resource_floor",
            Self::ControllerInit => "controller_init",
            Self::HostExited => "host_exited",
            Self::CallbackQuiescence => "callback_quiescence",
            Self::LateCallback => "late_callback",
            Self::PacketLeak => "packet_leak",
            Self::QueueLifecycle => "queue_lifecycle",
            Self::TransportFault => "transport_fault",
            Self::PacketExhausted => "packet_exhausted",
            Self::HostInit => "host_init",
            Self::AddressGeneration => "address_generation",
            Self::GattServerInit => "gatt_server_init",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Phase1dStatus {
    state: ProbeState,
    pub(crate) cycle: u8,
    failure: ProbeFailure,
    pub(crate) build_id: &'static str,
    pub(crate) cycles: u8,
    pub(crate) boot_generation: u32,
    pub(crate) epoch: u32,
    pub(crate) before_free: u32,
    pub(crate) controller_free: u32,
    pub(crate) active_free: u32,
    pub(crate) after_free: u32,
    pub(crate) callback_admission: bool,
    pub(crate) callbacks_in_flight: u32,
    pub(crate) callbacks_rejected: u32,
    pub(crate) rx_queue_overflow: u32,
    pub(crate) rx_oversize: u32,
    pub(crate) tx_rejected: u32,
    pub(crate) tx_timeout: u32,
    pub(crate) queues_active: u32,
    pub(crate) queue_late_use: u32,
    pub(crate) queue_unknown_use: u32,
    pub(crate) queue_reclaim_failures: u32,
    pub(crate) queue_corruption: u32,
    pub(crate) queue_contention: u32,
    pub(crate) queue_task_cancelled: u32,
    pub(crate) queue_operation_balance_error: u32,
    pub(crate) queue_task_live: u32,
    pub(crate) queue_task_faults: u32,
    pub(crate) queue_operation_registry_full: u32,
    pub(crate) transport_faulted: bool,
    pub(crate) packets_free: u8,
    pub(crate) pool_exhausted: u32,
}

impl Phase1dStatus {
    pub(crate) const fn state_label(self) -> &'static str {
        self.state.label()
    }

    pub(crate) const fn failure_label(self) -> &'static str {
        self.failure.label()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProbeRequestError {
    Busy,
    Consumed,
    OwnershipUnknown,
    ExclusiveLease,
    UpdateReserved,
}

/// BLE's hold on the radio. Defined by the arbiter so the supervisor can read
/// it without depending on this module (ADR-0015).
pub(crate) use arbitration::claim::Ownership as Phase1sOwnership;

pub(crate) fn phase1d_status() -> Phase1dStatus {
    let callbacks = hci_callback_stats();
    let transport = hci_transport_stats();
    let queues = esp_radio::queue_lifecycle_stats();
    Phase1dStatus {
        state: ProbeState::from_raw(PHASE1D_STATE.load(Ordering::Acquire)),
        cycle: PHASE1D_CYCLE.load(Ordering::Relaxed),
        failure: ProbeFailure::from_raw(PHASE1D_FAILURE.load(Ordering::Relaxed)),
        build_id: BUILD_ID,
        cycles: PHASE1S_CYCLES,
        boot_generation: PHASE1S_BOOT.load(Ordering::Relaxed),
        epoch: PHASE1S_EPOCH.load(Ordering::Relaxed),
        before_free: PHASE1S_BEFORE_FREE.load(Ordering::Relaxed),
        controller_free: PHASE1S_CONTROLLER_FREE.load(Ordering::Relaxed),
        active_free: PHASE1S_ACTIVE_FREE.load(Ordering::Relaxed),
        after_free: PHASE1S_AFTER_FREE.load(Ordering::Relaxed),
        callback_admission: callbacks.admission_open,
        callbacks_in_flight: callbacks.in_flight,
        callbacks_rejected: PHASE1S_CALLBACKS_REJECTED.load(Ordering::Relaxed),
        rx_queue_overflow: PHASE1S_RX_QUEUE_OVERFLOW.load(Ordering::Relaxed),
        rx_oversize: PHASE1S_RX_OVERSIZE.load(Ordering::Relaxed),
        tx_rejected: PHASE1S_TX_REJECTED.load(Ordering::Relaxed),
        tx_timeout: PHASE1S_TX_TIMEOUT.load(Ordering::Relaxed),
        queues_active: queues
            .active
            .saturating_add(queues.retired)
            .saturating_add(queues.owner_active)
            .saturating_add(queues.owner_retired)
            .min(u32::MAX as usize) as u32,
        queue_late_use: queues.late_use_rejected.min(u32::MAX as usize) as u32,
        queue_unknown_use: queues.unknown_use_rejected.min(u32::MAX as usize) as u32,
        queue_reclaim_failures: queues.reclaim_failures.min(u32::MAX as usize) as u32,
        queue_corruption: queues.owner_corruption.min(u32::MAX as usize) as u32,
        queue_contention: queues
            .owner_task_contention_rejected
            .saturating_add(queues.owner_isr_contention_rejected)
            .min(u32::MAX as usize) as u32,
        queue_task_cancelled: PHASE1S_QUEUE_TASK_CANCELLED.load(Ordering::Relaxed),
        queue_operation_balance_error: queues.operation_balance_error.min(u32::MAX as usize) as u32,
        queue_task_live: queues.btdm_task_live.min(u32::MAX as usize) as u32,
        queue_task_faults: queues
            .btdm_task_registry_failures
            .saturating_add(queues.btdm_task_delete_unattributed)
            .min(u32::MAX as usize) as u32,
        queue_operation_registry_full: queues.operation_registry_full.min(u32::MAX as usize) as u32,
        transport_faulted: transport.faulted,
        packets_free: Phase1PacketPool::free_count().min(u8::MAX as usize) as u8,
        pool_exhausted: Phase1PacketPool::exhausted_count(),
    }
}

pub(crate) fn request_phase1s_probe(
    boot_generation: u32,
    epoch: u32,
) -> Result<(), ProbeRequestError> {
    if !arbitration::claim::exclusive_lease_matches(boot_generation, epoch) {
        return Err(ProbeRequestError::ExclusiveLease);
    }
    if update_reserved() {
        return Err(ProbeRequestError::UpdateReserved);
    }
    let _ = PHASE1S_CLOSE_REQUEST.try_take();
    critical_section::with(|_| {
        let raw = PHASE1D_STATE.load(Ordering::Acquire);
        let state = ProbeState::from_raw(raw);
        match state {
            ProbeState::Queued | ProbeState::Running => return Err(ProbeRequestError::Busy),
            ProbeState::OwnershipUnknown => return Err(ProbeRequestError::OwnershipUnknown),
            ProbeState::Completed | ProbeState::Failed
                if PHASE1S_BOOT.load(Ordering::Relaxed) == boot_generation
                    && PHASE1S_EPOCH.load(Ordering::Relaxed) == epoch =>
            {
                return Err(ProbeRequestError::Consumed);
            }
            ProbeState::Idle | ProbeState::Completed | ProbeState::Failed => {}
        }
        PHASE1D_CYCLE.store(0, Ordering::Relaxed);
        PHASE1D_FAILURE.store(ProbeFailure::None as u8, Ordering::Relaxed);
        PHASE1S_BOOT.store(boot_generation, Ordering::Relaxed);
        PHASE1S_EPOCH.store(epoch, Ordering::Relaxed);
        PHASE1S_BEFORE_FREE.store(0, Ordering::Relaxed);
        PHASE1S_CONTROLLER_FREE.store(0, Ordering::Relaxed);
        PHASE1S_ACTIVE_FREE.store(0, Ordering::Relaxed);
        PHASE1S_AFTER_FREE.store(0, Ordering::Relaxed);
        PHASE1S_CALLBACKS_REJECTED.store(0, Ordering::Relaxed);
        PHASE1S_RX_QUEUE_OVERFLOW.store(0, Ordering::Relaxed);
        PHASE1S_RX_OVERSIZE.store(0, Ordering::Relaxed);
        PHASE1S_TX_REJECTED.store(0, Ordering::Relaxed);
        PHASE1S_TX_TIMEOUT.store(0, Ordering::Relaxed);
        PHASE1S_QUEUE_TASK_CANCELLED.store(0, Ordering::Relaxed);
        PHASE1D_STATE.store(ProbeState::Queued as u8, Ordering::Release);
        publish_ble_ownership();
        Ok(())
    })?;
    PHASE1S_REQUEST.signal(ProbeRequest {
        boot_generation,
        epoch,
    });
    Ok(())
}

/// Mirror this module's probe state into the arbitration claim. Called after
/// every `PHASE1D_STATE` transition; the supervisor reads the claim rather than
/// calling in here, which is what broke the `ble` <-> `net` cycle (ADR-0015).
fn publish_ble_ownership() {
    arbitration::claim::set_ble_ownership(phase1s_ownership());
}

fn phase1s_ownership() -> Phase1sOwnership {
    match ProbeState::from_raw(PHASE1D_STATE.load(Ordering::Acquire)) {
        ProbeState::Queued | ProbeState::Running => Phase1sOwnership::Active,
        ProbeState::OwnershipUnknown => Phase1sOwnership::Unknown,
        ProbeState::Idle | ProbeState::Completed | ProbeState::Failed => {
            Phase1sOwnership::KnownClosed
        }
    }
}

fn update_reserved() -> bool {
    crate::firmware::update::transport_quiet()
}

#[derive(Clone, Copy)]
struct SampleResult {
    exclusive_ok: bool,
    resource_ok: bool,
    internal_free: u32,
}

impl SampleResult {
    const fn failed() -> Self {
        Self {
            exclusive_ok: false,
            resource_ok: false,
            internal_free: 0,
        }
    }
}

fn log_phase1s_sample(stage: &'static str, request: ProbeRequest) -> SampleResult {
    crate::firmware::observability::record_stack_headroom();
    let heap = crate::firmware::psram::allocator_memory_snapshot();
    let main_stack = crate::firmware::observability::minimum_stack_headroom_bytes();
    let touch_stack = crate::firmware::observability::minimum_touch_core_stack_headroom_bytes();
    let observed = arbitration::claim::observations();
    let callbacks = hci_callback_stats();
    let transport = hci_transport_stats();
    // The exact off lease is the supervisor's ownership proof. `radio_quiesced`
    // describes the connection task's intentional-dormant policy and is not
    // updated when the supervisor destroys a complete Wi-Fi epoch.
    // One question, asked of the arbiter. This used to be assembled here from
    // four separate reaches into the network supervisor, which is what made
    // `ble` and `net` mutually dependent (ADR-0015).
    let exclusive_ok =
        arbitration::claim::exclusive_ownership_confirmed(request.boot_generation, request.epoch);
    let resource_ready =
        main_stack >= 8_192 && touch_stack >= 1_024 && heap.free_internal_bytes >= 16_384;
    console::println!(
        "BLE_PHASE1S sample stage={} boot={} epoch={} coex=false wifi_controller={} net_runner={} wifi_link={} radio_quiesced={} listener={} internal_free={} internal_min={} cpu0_stack_min={} touch_stack_min={} callback_admission={} callback_in_flight={} callback_accepted={} callback_rejected={} callback_high_water={} rx_queue_high_water={} rx_queue_overflow={} rx_oversize={} tx_rejected={} tx_timeout={} transport_faulted={} packets_free={} pool_exhausted={} exclusive_ok={} resource_ok={}",
        stage,
        request.boot_generation,
        request.epoch,
        observed.wifi_controller_resident,
        observed.net_runner_resident,
        observed.wifi_link,
        observed.radio_quiesced,
        observed.service_listening,
        heap.free_internal_bytes,
        heap.min_free_internal_bytes,
        main_stack,
        touch_stack,
        callbacks.admission_open,
        callbacks.in_flight,
        callbacks.accepted,
        callbacks.rejected,
        callbacks.high_water,
        transport.rx_queue_high_water,
        transport.rx_queue_overflow,
        transport.rx_oversize,
        transport.tx_rejected,
        transport.tx_timeout,
        transport.faulted,
        Phase1PacketPool::free_count(),
        Phase1PacketPool::exhausted_count(),
        exclusive_ok,
        resource_ready,
    );
    SampleResult {
        exclusive_ok,
        resource_ok: resource_ready,
        internal_free: heap.free_internal_bytes.min(u32::MAX as usize) as u32,
    }
}

fn current_internal_free() -> u32 {
    crate::firmware::psram::allocator_memory_snapshot()
        .free_internal_bytes
        .min(u32::MAX as usize) as u32
}

/// The current tick, in `ble::deadline`/`ble::diagnostic::EchoRateLimiter`'s
/// shared unit (`ONE_SECOND_TICKS` microseconds per second).
fn now_micros() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

/// `ble::deadline::VisibilityWindow::remaining_ticks`'s result, converted to
/// an `embassy_time::Duration` to race against. Zero ticks still yields a
/// zero-length timer rather than panicking or blocking -- `Timer::after`
/// with a zero duration fires on the next poll.
fn ticks_to_timeout(remaining_ticks: u64) -> Duration {
    Duration::from_micros(remaining_ticks)
}
