//! Non-default BLE foundation probe.
//!
//! Phase 1 deliberately exposes only build identity, lifecycle state, and a
//! bounded echo value. Product service policy belongs to later phases.

// trouble-host 0.6 derive expansions trigger this lint at unchanged field-type
// spans; keep the exception inside the non-default probe module.
#![allow(clippy::needless_borrows_for_generic_args)]

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use ::embassy_sync as embassy_sync_08;
use bt_hci::cmd::info::ReadBdAddr;
use embassy_futures::select::{select, Either};
use embassy_sync_08::{
    blocking_mutex::raw::CriticalSectionRawMutex as CriticalSectionRawMutex08, signal::Signal,
};
use embassy_time::{Duration, Timer};
use esp_radio::ble::{
    begin_hci_callback_shutdown, controller::BleConnector, hci_callback_stats, hci_transport_stats,
    wait_for_hci_callback_quiescence, HciTransportStats,
};
use esp_radio::QueueLifecycleStats;
use trouble_host::prelude::*;

mod packet_pool;
mod reusable_slot;

use packet_pool::{free_packet_count, pool_exhausted_count, Phase1PacketPool};
use reusable_slot::ReusableSlot;

const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 2;
const PACKET_MTU: usize = 64;
const PACKET_COUNT: usize = 4;
const PHASE1S_CYCLES: u8 = 1;
const REQUIRED_PREINIT_FREE: usize = 20_496;
const REQUIRED_PREINIT_BLOCK: usize = 4_112;
const INTERNAL_RESERVE: usize = 16_384;
const ACTIVE_WINDOW: Duration = Duration::from_millis(750);
const HOST_INIT_TIMEOUT: Duration = Duration::from_secs(2);
const CALLBACK_QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(2);
const CALLBACK_QUIET_DWELL: Duration = Duration::from_millis(20);
const BUILD_ID: &str = match option_env!("MEDITAMER_FIRMWARE_BUILD_ID") {
    Some(value) => value,
    None => "unlabeled",
};

type Controller = ExternalController<BleConnector<'static>, 1>;
type Resources = HostResources<Phase1PacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX>;
type Phase1Stack = Stack<'static, Controller, Phase1PacketPool>;

static PHASE1S_REQUEST: Signal<CriticalSectionRawMutex08, ProbeRequest> = Signal::new();
static PHASE1S_CLOSE_REQUEST: Signal<CriticalSectionRawMutex08, ()> = Signal::new();
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
static HOST_STACK: ReusableSlot<Phase1Stack> = ReusableSlot::new();

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase1sOwnership {
    KnownClosed,
    Active,
    Unknown,
}

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
        packets_free: free_packet_count().min(u8::MAX as usize) as u8,
        pool_exhausted: pool_exhausted_count(),
    }
}

pub(crate) fn request_phase1s_probe(
    boot_generation: u32,
    epoch: u32,
) -> Result<(), ProbeRequestError> {
    if !crate::firmware::net::exclusive_lease_matches(boot_generation, epoch) {
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
        Ok(())
    })?;
    PHASE1S_REQUEST.signal(ProbeRequest {
        boot_generation,
        epoch,
    });
    Ok(())
}

pub(crate) fn phase1s_ownership() -> Phase1sOwnership {
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

#[embassy_executor::task]
pub(crate) async fn phase1_task(bluetooth: esp_hal::peripherals::BT<'static>) {
    console::println!(
        "BLE_PHASE1S state=armed build_id={} cycles={} packet_mtu={} packet_count={} coex=false",
        BUILD_ID,
        PHASE1S_CYCLES,
        PACKET_MTU,
        PACKET_COUNT,
    );
    let mut first_device = Some(bluetooth);
    loop {
        let request = PHASE1S_REQUEST.wait().await;
        if PHASE1S_CLOSE_REQUEST.try_take().is_some() || update_reserved() {
            PHASE1D_FAILURE.store(ProbeFailure::UpdateReserved as u8, Ordering::Relaxed);
            PHASE1D_STATE.store(ProbeState::Failed as u8, Ordering::Release);
            continue;
        }
        PHASE1D_STATE.store(ProbeState::Running as u8, Ordering::Release);
        let completion = run_phase1s_probe(&mut first_device, request).await;
        PHASE1D_FAILURE.store(completion.failure as u8, Ordering::Relaxed);
        PHASE1D_STATE.store(completion.state as u8, Ordering::Release);
        console::println!(
            "BLE_PHASE1S state={} boot={} epoch={} failure={}",
            completion.state.label(),
            request.boot_generation,
            request.epoch,
            completion.failure.label(),
        );
    }
}

#[derive(Clone, Copy)]
struct ProbeCompletion {
    state: ProbeState,
    failure: ProbeFailure,
}

async fn run_phase1s_probe(
    first_device: &mut Option<esp_hal::peripherals::BT<'static>>,
    request: ProbeRequest,
) -> ProbeCompletion {
    let preinit = crate::firmware::psram::probe_internal_block_above_reserve(INTERNAL_RESERVE);
    if !preinit.stable
        || preinit.free_before_bytes < REQUIRED_PREINIT_FREE
        || preinit.free_after_bytes < REQUIRED_PREINIT_FREE
        || preinit.block_bytes < REQUIRED_PREINIT_BLOCK
    {
        return ProbeCompletion {
            state: ProbeState::Failed,
            failure: ProbeFailure::ResourceFloor,
        };
    }
    let before = log_phase1s_sample("before", request);
    PHASE1S_BEFORE_FREE.store(before.internal_free, Ordering::Relaxed);
    if !before.exclusive_ok {
        return ProbeCompletion {
            state: ProbeState::Failed,
            failure: ProbeFailure::ExclusiveLease,
        };
    }
    if !before.resource_ok {
        return ProbeCompletion {
            state: ProbeState::Failed,
            failure: ProbeFailure::ResourceFloor,
        };
    }

    PHASE1D_CYCLE.store(1, Ordering::Relaxed);
    let device = first_device.take().unwrap_or_else(|| {
        // The prior exclusive window disabled the callback source, observed
        // quiescence, and dropped the connector that owned BT.
        unsafe { esp_hal::peripherals::BT::steal() }
    });
    if let Err(failure) = run_phase1s_cycle(device, request).await {
        return ProbeCompletion {
            state: if matches!(
                failure,
                ProbeFailure::CallbackQuiescence
                    | ProbeFailure::LateCallback
                    | ProbeFailure::PacketLeak
                    | ProbeFailure::QueueLifecycle
                    | ProbeFailure::TransportFault
                    | ProbeFailure::ControllerInit
            ) {
                ProbeState::OwnershipUnknown
            } else {
                ProbeState::Failed
            },
            failure,
        };
    }

    ProbeCompletion {
        state: ProbeState::Completed,
        failure: ProbeFailure::None,
    }
}

async fn run_phase1s_cycle(
    device: esp_hal::peripherals::BT<'static>,
    request: ProbeRequest,
) -> Result<(), ProbeFailure> {
    let cycle = 1;
    let queue_task_cancelled_before =
        esp_radio::queue_lifecycle_stats().operation_cancelled_on_task_delete;
    if !crate::firmware::net::exclusive_lease_matches(request.boot_generation, request.epoch) {
        return Err(ProbeFailure::ExclusiveLease);
    }
    if update_reserved() {
        return Err(ProbeFailure::UpdateReserved);
    }
    let connector = BleConnector::new(device, Default::default()).map_err(|error| {
        console::println!(
            "BLE_PHASE1D state=init_error cycle={} error={:?}",
            cycle,
            error
        );
        ProbeFailure::ControllerInit
    })?;
    PHASE1S_CONTROLLER_FREE.store(current_internal_free(), Ordering::Relaxed);
    let controller: Controller = ExternalController::new(connector);
    let resources = HOST_RESOURCES.initialize(Resources::new());
    let stack = HOST_STACK.initialize(
        trouble_host::new(controller, resources)
            .set_random_address(Address::random([0xff, 0x4e, 0x3d, 0x2c, 0x1b, cycle])),
    );
    let Host {
        peripheral, runner, ..
    } = stack.build();
    let (host_failure, active_sample) = {
        // Keep the pinned Runner future inside this block. Its borrow of HOST_STACK
        // must end before the callback fence and unsafe reusable-slot clear below.
        let mut host = core::pin::pin!(run_host(runner));
        let initialized =
            embassy_time::with_timeout(HOST_INIT_TIMEOUT, stack.command(ReadBdAddr::new()));
        match select(host.as_mut(), initialized).await {
            Either::First(_) => (Some(ProbeFailure::HostExited), SampleResult::failed()),
            Either::Second(Ok(Ok(_))) => {
                let close = close_phase1s_window(request);
                match select(host.as_mut(), close).await {
                    Either::First(_) => (Some(ProbeFailure::HostExited), SampleResult::failed()),
                    Either::Second(sample) => (None, sample),
                }
            }
            Either::Second(Ok(Err(_)) | Err(_)) => {
                (Some(ProbeFailure::HostInit), SampleResult::failed())
            }
        }
    };
    begin_hci_callback_shutdown();
    let before_drop = hci_callback_stats();

    if !wait_for_hci_callback_quiescence(CALLBACK_QUIESCENCE_TIMEOUT).await {
        console::println!(
            "BLE_PHASE1D state=close_timeout cycle={} callbacks_in_flight={}",
            cycle,
            hci_callback_stats().in_flight,
        );
        return Err(ProbeFailure::CallbackQuiescence);
    }
    let quiescent = hci_callback_stats();
    let transport = hci_transport_stats();
    PHASE1S_CALLBACKS_REJECTED.store(quiescent.rejected, Ordering::Relaxed);
    PHASE1S_RX_QUEUE_OVERFLOW.store(transport.rx_queue_overflow, Ordering::Relaxed);
    PHASE1S_RX_OVERSIZE.store(transport.rx_oversize, Ordering::Relaxed);
    PHASE1S_TX_REJECTED.store(transport.tx_rejected, Ordering::Relaxed);
    PHASE1S_TX_TIMEOUT.store(transport.tx_timeout, Ordering::Relaxed);
    cancel_host_parts(peripheral);
    unsafe {
        HOST_STACK.clear();
        HOST_RESOURCES.clear();
    }
    Timer::after(CALLBACK_QUIET_DWELL).await;
    let teardown_transport = hci_transport_stats();
    let settled = hci_callback_stats();
    if settled.in_flight != 0
        || settled.accepted != quiescent.accepted
        || settled.rejected != quiescent.rejected
    {
        return Err(ProbeFailure::LateCallback);
    }
    if quiescent.rejected != 0 {
        return Err(ProbeFailure::LateCallback);
    }
    if free_packet_count() != PACKET_COUNT {
        return Err(ProbeFailure::PacketLeak);
    }
    let queue_task_cancelled = validate_queue_teardown(
        esp_radio::queue_lifecycle_stats(),
        queue_task_cancelled_before,
    )?;
    PHASE1S_QUEUE_TASK_CANCELLED.store(
        queue_task_cancelled.min(u32::MAX as usize) as u32,
        Ordering::Relaxed,
    );
    validate_transport_teardown(transport, teardown_transport)?;
    if pool_exhausted_count() != 0 {
        return Err(ProbeFailure::PacketExhausted);
    }
    console::println!(
        "BLE_PHASE1D close cycle={} deadline_ms=2000 pre_in_flight={} accepted={} rejected={} callback_high_water={} settled_in_flight={} rx_queue_high_water={} rx_queue_overflow={} rx_oversize={} tx_rejected={} tx_timeout={} transport_faulted={} packets_free={} pool_exhausted={}",
        cycle,
        before_drop.in_flight,
        settled.accepted,
        settled.rejected,
        settled.high_water,
        settled.in_flight,
        transport.rx_queue_high_water,
        transport.rx_queue_overflow,
        transport.rx_oversize,
        transport.tx_rejected,
        transport.tx_timeout,
        transport.faulted,
        free_packet_count(),
        pool_exhausted_count(),
    );
    if let Some(failure) = host_failure {
        return Err(failure);
    }
    let after = log_phase1s_sample("after", request);
    PHASE1S_AFTER_FREE.store(after.internal_free, Ordering::Relaxed);
    if !active_sample.exclusive_ok || !after.exclusive_ok {
        return Err(ProbeFailure::ExclusiveLease);
    }
    if !active_sample.resource_ok || !after.resource_ok {
        return Err(ProbeFailure::ResourceFloor);
    }
    Ok(())
}

fn validate_queue_teardown(
    queues: QueueLifecycleStats,
    cancelled_before: usize,
) -> Result<usize, ProbeFailure> {
    let cancelled = queues
        .operation_cancelled_on_task_delete
        .checked_sub(cancelled_before)
        .ok_or(ProbeFailure::QueueLifecycle)?;
    if queues.active != 0
        || queues.retired != 0
        || queues.owner_active != 0
        || queues.owner_retired != 0
        || queues.late_use_rejected != 0
        || queues.unknown_use_rejected != 0
        || queues.reclaim_failures != 0
        || queues.owner_corruption != 0
        || queues.owner_task_contention_rejected != 0
        || queues.owner_isr_contention_rejected != 0
        || cancelled > queues.slot_capacity
        || queues.operation_balance_error != 0
        || queues.operation_registry_full != 0
        || queues.btdm_task_live != 0
        || queues.btdm_task_registry_failures != 0
        || queues.btdm_task_delete_unattributed != 0
    {
        return Err(ProbeFailure::QueueLifecycle);
    }
    Ok(cancelled)
}

fn validate_transport_teardown(
    active: HciTransportStats,
    teardown: HciTransportStats,
) -> Result<(), ProbeFailure> {
    if active.faulted
        || teardown.faulted
        || active.rx_queue_overflow != 0
        || active.rx_oversize != 0
        || active.tx_rejected != 0
        || active.tx_timeout != 0
    {
        return Err(ProbeFailure::TransportFault);
    }
    Ok(())
}

fn cancel_host_parts<T>(_parts: T) {}

async fn close_phase1s_window(request: ProbeRequest) -> SampleResult {
    let _ = select(Timer::after(ACTIVE_WINDOW), PHASE1S_CLOSE_REQUEST.wait()).await;
    let sample = log_phase1s_sample("active", request);
    PHASE1S_ACTIVE_FREE.store(sample.internal_free, Ordering::Relaxed);
    begin_hci_callback_shutdown();
    sample
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
    let residency = crate::firmware::net::residency_snapshot();
    let net = crate::firmware::net::wifi::net_status_snapshot();
    let callbacks = hci_callback_stats();
    let transport = hci_transport_stats();
    // The exact off lease is the supervisor's ownership proof. `radio_quiesced`
    // describes the connection task's intentional-dormant policy and is not
    // updated when the supervisor destroys a complete Wi-Fi epoch.
    let exclusive_ok = crate::firmware::net::phase1s_exclusive_ownership_confirmed(
        crate::firmware::net::exclusive_lease_matches(request.boot_generation, request.epoch),
        residency.wifi_controller_task,
        residency.net_runner_task,
        net.link,
        net.listener,
    );
    let resource_ready =
        main_stack >= 8_192 && touch_stack >= 1_024 && heap.free_internal_bytes >= 16_384;
    console::println!(
        "BLE_PHASE1S sample stage={} boot={} epoch={} coex=false wifi_controller={} net_runner={} wifi_link={} radio_quiesced={} listener={} internal_free={} internal_min={} cpu0_stack_min={} touch_stack_min={} callback_admission={} callback_in_flight={} callback_accepted={} callback_rejected={} callback_high_water={} rx_queue_high_water={} rx_queue_overflow={} rx_oversize={} tx_rejected={} tx_timeout={} transport_faulted={} packets_free={} pool_exhausted={} exclusive_ok={} resource_ok={}",
        stage,
        request.boot_generation,
        request.epoch,
        residency.wifi_controller_task,
        residency.net_runner_task,
        net.link,
        net.radio_quiesced,
        net.listener,
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
        free_packet_count(),
        pool_exhausted_count(),
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

async fn run_host(mut runner: Runner<'_, Controller, Phase1PacketPool>) {
    if let Err(error) = runner.run().await {
        console::println!(
            "BLE_PHASE1 state=host_error error={:?} pool_exhausted={}",
            error,
            pool_exhausted_count()
        );
    }

    // Return to the lifecycle owner. It revokes callback ingress, tears down
    // the controller, and requires a complete reinitialization before reuse.
}
