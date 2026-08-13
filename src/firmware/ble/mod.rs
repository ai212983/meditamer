//! Non-default BLE foundation probe.
//!
//! Phase 1 deliberately exposes only build identity, lifecycle state, and a
//! bounded echo value. Product service policy belongs to later phases.

// trouble-host 0.6 derive expansions trigger this lint at unchanged field-type
// spans; keep the exception inside the non-default probe module.
#![allow(clippy::needless_borrows_for_generic_args)]

use core::sync::atomic::{AtomicU8, Ordering};

use ::embassy_sync as embassy_sync_08;
use embassy_futures::select::{select, Either};
use embassy_sync_07 as embassy_sync;
use embassy_sync_08::{
    blocking_mutex::raw::CriticalSectionRawMutex as CriticalSectionRawMutex08, signal::Signal,
};
use embassy_time::{Duration, Timer};
use esp_radio::ble::{
    begin_hci_callback_shutdown, controller::BleConnector, hci_callback_stats, hci_transport_stats,
    wait_for_hci_callback_quiescence,
};
use trouble_host::prelude::*;

mod packet_pool;
mod reusable_slot;

use packet_pool::{free_packet_count, pool_exhausted_count, Phase1PacketPool};
use reusable_slot::ReusableSlot;

const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 2;
const PACKET_MTU: usize = 64;
const PACKET_COUNT: usize = 4;
const PHASE1D_CYCLES: u8 = 20;
const ACTIVE_WINDOW: Duration = Duration::from_millis(750);
const CALLBACK_QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(2);
const CALLBACK_QUIET_DWELL: Duration = Duration::from_millis(20);
const BUILD_ID: &str = match option_env!("MEDITAMER_FIRMWARE_BUILD_ID") {
    Some(value) => value,
    None => "unlabeled",
};

type Controller = ExternalController<BleConnector<'static>, 1>;
type Resources = HostResources<Phase1PacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX>;
type Phase1Stack = Stack<'static, Controller, Phase1PacketPool>;

static PHASE1D_REQUEST: Signal<CriticalSectionRawMutex08, ()> = Signal::new();
static PHASE1D_STATE: AtomicU8 = AtomicU8::new(ProbeState::Idle as u8);
static PHASE1D_CYCLE: AtomicU8 = AtomicU8::new(0);
static PHASE1D_FAILURE: AtomicU8 = AtomicU8::new(ProbeFailure::None as u8);
static HOST_RESOURCES: ReusableSlot<Resources> = ReusableSlot::new();
static HOST_STACK: ReusableSlot<Phase1Stack> = ReusableSlot::new();
static GATT_SERVER: ReusableSlot<Server<'static>> = ReusableSlot::new();

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
    WifiPrerequisite,
    ResourceFloor,
    ControllerInit,
    GattInit,
    HostExited,
    CallbackQuiescence,
    LateCallback,
    PacketLeak,
}

impl ProbeFailure {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::WifiPrerequisite,
            2 => Self::ResourceFloor,
            3 => Self::ControllerInit,
            4 => Self::GattInit,
            5 => Self::HostExited,
            6 => Self::CallbackQuiescence,
            7 => Self::LateCallback,
            8 => Self::PacketLeak,
            _ => Self::None,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::WifiPrerequisite => "wifi_prerequisite",
            Self::ResourceFloor => "resource_floor",
            Self::ControllerInit => "controller_init",
            Self::GattInit => "gatt_init",
            Self::HostExited => "host_exited",
            Self::CallbackQuiescence => "callback_quiescence",
            Self::LateCallback => "late_callback",
            Self::PacketLeak => "packet_leak",
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
    OwnershipUnknown,
}

pub(crate) fn phase1d_status() -> Phase1dStatus {
    Phase1dStatus {
        state: ProbeState::from_raw(PHASE1D_STATE.load(Ordering::Acquire)),
        cycle: PHASE1D_CYCLE.load(Ordering::Relaxed),
        failure: ProbeFailure::from_raw(PHASE1D_FAILURE.load(Ordering::Relaxed)),
        build_id: BUILD_ID,
        cycles: PHASE1D_CYCLES,
    }
}

pub(crate) fn request_phase1d_probe() -> Result<(), ProbeRequestError> {
    loop {
        let raw = PHASE1D_STATE.load(Ordering::Acquire);
        let state = ProbeState::from_raw(raw);
        match state {
            ProbeState::Queued | ProbeState::Running => return Err(ProbeRequestError::Busy),
            ProbeState::OwnershipUnknown => return Err(ProbeRequestError::OwnershipUnknown),
            ProbeState::Idle | ProbeState::Completed | ProbeState::Failed => {}
        }
        if PHASE1D_STATE
            .compare_exchange(
                raw,
                ProbeState::Queued as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            PHASE1D_CYCLE.store(0, Ordering::Relaxed);
            PHASE1D_FAILURE.store(ProbeFailure::None as u8, Ordering::Relaxed);
            PHASE1D_REQUEST.signal(());
            return Ok(());
        }
    }
}

#[gatt_server(
    connections_max = CONNECTIONS_MAX,
    mutex_type = embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    packet_type = Phase1PacketPool,
    attribute_table_size = 24
)]
struct Server {
    foundation: FoundationService,
}

#[gatt_service(uuid = "7b4e0001-25c5-4e58-9e5e-7a1f0b2c3d4e")]
struct FoundationService {
    #[characteristic(uuid = "7b4e0002-25c5-4e58-9e5e-7a1f0b2c3d4e", read, value = BUILD_ID)]
    build_id: &'static str,
    #[characteristic(uuid = "7b4e0003-25c5-4e58-9e5e-7a1f0b2c3d4e", read, write, notify)]
    echo: heapless::Vec<u8, 32>,
    #[characteristic(uuid = "7b4e0004-25c5-4e58-9e5e-7a1f0b2c3d4e", read, notify, value = 0)]
    lifecycle: u8,
}

#[embassy_executor::task]
pub(crate) async fn phase1_task(bluetooth: esp_hal::peripherals::BT<'static>) {
    esp_println::println!(
        "BLE_PHASE1D state=armed build_id={} cycles={} packet_mtu={} packet_count={} coex={}",
        BUILD_ID,
        PHASE1D_CYCLES,
        PACKET_MTU,
        PACKET_COUNT,
        cfg!(feature = "ble-foundation"),
    );
    let mut first_device = Some(bluetooth);
    loop {
        PHASE1D_REQUEST.wait().await;
        PHASE1D_STATE.store(ProbeState::Running as u8, Ordering::Release);
        let completion = run_phase1d_probe(&mut first_device).await;
        PHASE1D_FAILURE.store(completion.failure as u8, Ordering::Relaxed);
        PHASE1D_STATE.store(completion.state as u8, Ordering::Release);
        esp_println::println!(
            "BLE_PHASE1D state={} cycle={} failure={}",
            completion.state.label(),
            PHASE1D_CYCLE.load(Ordering::Relaxed),
            completion.failure.label(),
        );
    }
}

#[derive(Clone, Copy)]
struct ProbeCompletion {
    state: ProbeState,
    failure: ProbeFailure,
}

async fn run_phase1d_probe(
    first_device: &mut Option<esp_hal::peripherals::BT<'static>>,
) -> ProbeCompletion {
    let before = log_phase1d_sample("before", 0);
    if !before.wifi_ok {
        return ProbeCompletion {
            state: ProbeState::Failed,
            failure: ProbeFailure::WifiPrerequisite,
        };
    }
    if !before.resource_ok {
        return ProbeCompletion {
            state: ProbeState::Failed,
            failure: ProbeFailure::ResourceFloor,
        };
    }

    for cycle in 1..=PHASE1D_CYCLES {
        PHASE1D_CYCLE.store(cycle, Ordering::Relaxed);
        let device = first_device.take().unwrap_or_else(|| {
            // Each prior cycle disabled the controller callback source, observed
            // callback quiescence, and dropped the connector that owned BT.
            // `steal` reconstructs that singleton token for the next bounded
            // reinitialization cycle; OwnershipUnknown permanently blocks this path.
            unsafe { esp_hal::peripherals::BT::steal() }
        });
        match run_phase1d_cycle(device, cycle).await {
            Ok(()) => {}
            Err(failure) => {
                return ProbeCompletion {
                    state: if matches!(
                        failure,
                        ProbeFailure::CallbackQuiescence
                            | ProbeFailure::LateCallback
                            | ProbeFailure::PacketLeak
                    ) {
                        ProbeState::OwnershipUnknown
                    } else {
                        ProbeState::Failed
                    },
                    failure,
                };
            }
        }
    }

    ProbeCompletion {
        state: ProbeState::Completed,
        failure: ProbeFailure::None,
    }
}

async fn run_phase1d_cycle(
    device: esp_hal::peripherals::BT<'static>,
    cycle: u8,
) -> Result<(), ProbeFailure> {
    let connector = BleConnector::new(device, Default::default()).map_err(|error| {
        esp_println::println!(
            "BLE_PHASE1D state=init_error cycle={} error={:?}",
            cycle,
            error
        );
        ProbeFailure::ControllerInit
    })?;
    let controller: Controller = ExternalController::new(connector);
    let resources = HOST_RESOURCES.initialize(Resources::new());
    let stack = HOST_STACK.initialize(
        trouble_host::new(controller, resources)
            .set_random_address(Address::random([0xff, 0x4e, 0x3d, 0x2c, 0x1b, cycle])),
    );
    let Host {
        peripheral, runner, ..
    } = stack.build();
    let server = match initialize_gatt_server(cycle) {
        Some(server) => server,
        None => {
            begin_hci_callback_shutdown();
            cancel_host_parts((peripheral, runner));
            if !wait_for_hci_callback_quiescence(CALLBACK_QUIESCENCE_TIMEOUT).await {
                esp_println::println!(
                    "BLE_PHASE1D state=close_timeout cycle={} callbacks_in_flight={}",
                    cycle,
                    hci_callback_stats().in_flight,
                );
                return Err(ProbeFailure::CallbackQuiescence);
            }
            let quiescent = hci_callback_stats();
            unsafe {
                HOST_STACK.clear();
                HOST_RESOURCES.clear();
            }
            Timer::after(CALLBACK_QUIET_DWELL).await;
            let settled = hci_callback_stats();
            if settled.in_flight != 0
                || settled.accepted != quiescent.accepted
                || settled.rejected != quiescent.rejected
            {
                return Err(ProbeFailure::LateCallback);
            }
            if free_packet_count() != PACKET_COUNT {
                return Err(ProbeFailure::PacketLeak);
            }
            return Err(ProbeFailure::GattInit);
        }
    };

    let active = select(run_host(runner), run_service(peripheral, server));
    let close = close_phase1d_window(cycle);
    let (host_exited, active_sample) = match select(active, close).await {
        Either::First(_) => (true, SampleResult::failed()),
        Either::Second(sample) => (false, sample),
    };
    begin_hci_callback_shutdown();
    let before_drop = hci_callback_stats();

    if !wait_for_hci_callback_quiescence(CALLBACK_QUIESCENCE_TIMEOUT).await {
        esp_println::println!(
            "BLE_PHASE1D state=close_timeout cycle={} callbacks_in_flight={}",
            cycle,
            hci_callback_stats().in_flight,
        );
        return Err(ProbeFailure::CallbackQuiescence);
    }
    let quiescent = hci_callback_stats();
    let transport = hci_transport_stats();
    unsafe {
        GATT_SERVER.clear();
        HOST_STACK.clear();
        HOST_RESOURCES.clear();
    }
    Timer::after(CALLBACK_QUIET_DWELL).await;
    let settled = hci_callback_stats();
    if settled.in_flight != 0
        || settled.accepted != quiescent.accepted
        || settled.rejected != quiescent.rejected
    {
        return Err(ProbeFailure::LateCallback);
    }
    if free_packet_count() != PACKET_COUNT {
        return Err(ProbeFailure::PacketLeak);
    }
    esp_println::println!(
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
    if host_exited {
        return Err(ProbeFailure::HostExited);
    }
    let after = log_phase1d_sample("after", cycle);
    if !active_sample.wifi_ok || !after.wifi_ok {
        return Err(ProbeFailure::WifiPrerequisite);
    }
    if !active_sample.resource_ok || !after.resource_ok {
        return Err(ProbeFailure::ResourceFloor);
    }
    Ok(())
}

#[inline(never)]
fn initialize_gatt_server(cycle: u8) -> Option<&'static mut Server<'static>> {
    match Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: "Meditamer-P1",
        appearance: &appearance::UNKNOWN,
    })) {
        Ok(server) => Some(GATT_SERVER.initialize(server)),
        Err(error) => {
            esp_println::println!(
                "BLE_PHASE1D state=gatt_error cycle={} error={:?}",
                cycle,
                error
            );
            None
        }
    }
}

fn cancel_host_parts<T>(_parts: T) {}

async fn close_phase1d_window(cycle: u8) -> SampleResult {
    Timer::after(ACTIVE_WINDOW).await;
    let sample_ok = log_phase1d_sample("active", cycle);
    begin_hci_callback_shutdown();
    sample_ok
}

#[derive(Clone, Copy)]
struct SampleResult {
    wifi_ok: bool,
    resource_ok: bool,
}

impl SampleResult {
    const fn failed() -> Self {
        Self {
            wifi_ok: false,
            resource_ok: false,
        }
    }
}

fn log_phase1d_sample(stage: &'static str, cycle: u8) -> SampleResult {
    crate::firmware::observability::record_stack_headroom();
    let heap = crate::firmware::psram::allocator_memory_snapshot();
    let main_stack = crate::firmware::observability::minimum_stack_headroom_bytes();
    let touch_stack = crate::firmware::observability::minimum_touch_core_stack_headroom_bytes();
    let residency = crate::firmware::net::residency_snapshot();
    let net = crate::firmware::net::wifi::net_status_snapshot();
    let dhcp = net.ipv4 != [0, 0, 0, 0];
    let callbacks = hci_callback_stats();
    let transport = hci_transport_stats();
    let wifi_ready = residency.wifi_controller_task
        && residency.net_runner_task
        && net.link
        && dhcp
        && !net.radio_quiesced;
    let resource_ready =
        main_stack >= 8_192 && touch_stack >= 1_024 && heap.free_internal_bytes >= 16_384;
    esp_println::println!(
        "BLE_PHASE1D sample stage={} cycle={} coex={} wifi_controller={} net_runner={} wifi_link={} dhcp={} listener={} internal_free={} internal_min={} cpu0_stack_min={} touch_stack_min={} callback_admission={} callback_in_flight={} callback_accepted={} callback_rejected={} callback_high_water={} rx_queue_high_water={} rx_queue_overflow={} rx_oversize={} tx_rejected={} tx_timeout={} transport_faulted={} packets_free={} pool_exhausted={} wifi_ok={} resource_ok={}",
        stage,
        cycle,
        cfg!(feature = "ble-foundation"),
        residency.wifi_controller_task,
        residency.net_runner_task,
        net.link,
        dhcp,
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
        wifi_ready,
        resource_ready,
    );
    SampleResult {
        wifi_ok: wifi_ready,
        resource_ok: resource_ready,
    }
}

async fn run_host(mut runner: Runner<'_, Controller, Phase1PacketPool>) {
    if let Err(error) = runner.run().await {
        esp_println::println!(
            "BLE_PHASE1 state=host_error error={:?} pool_exhausted={}",
            error,
            pool_exhausted_count()
        );
    }

    // Return to the lifecycle owner. It revokes callback ingress, tears down
    // the controller, and requires a complete reinitialization before reuse.
}

async fn run_service<'stack, 'server>(
    mut peripheral: Peripheral<'stack, Controller, Phase1PacketPool>,
    server: &'server Server<'server>,
) {
    loop {
        match advertise(&mut peripheral, server).await {
            Ok(connection) => serve_connection(server, connection).await,
            Err(error) => {
                esp_println::println!("BLE_PHASE1 state=advertise_error error={:?}", error);
                Timer::after(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn advertise<'stack, 'server>(
    peripheral: &mut Peripheral<'stack, Controller, Phase1PacketPool>,
    server: &'server Server<'server>,
) -> Result<
    GattConnection<'stack, 'server, Phase1PacketPool>,
    BleHostError<<Controller as embedded_io_async::ErrorType>::Error>,
> {
    let mut advertisement_data = [0; 31];
    let advertisement_len = AdStructure::encode_slice(
        &[AdStructure::Flags(
            LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED,
        )],
        &mut advertisement_data,
    )?;
    let mut scan_data = [0; 31];
    let scan_len = AdStructure::encode_slice(
        &[AdStructure::CompleteLocalName(b"Meditamer-P1")],
        &mut scan_data,
    )?;
    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &advertisement_data[..advertisement_len],
                scan_data: &scan_data[..scan_len],
            },
        )
        .await?;
    esp_println::println!("BLE_PHASE1 state=advertising");
    let connection = advertiser.accept().await?.with_attribute_server(server)?;
    esp_println::println!("BLE_PHASE1 state=connected");
    Ok(connection)
}

async fn serve_connection<'stack, 'server>(
    server: &'server Server<'server>,
    connection: GattConnection<'stack, 'server, Phase1PacketPool>,
) {
    let lifecycle = server.foundation.lifecycle;
    let echo = &server.foundation.echo;
    let _ = server.set(&lifecycle, &1);
    let _ = lifecycle.notify(&connection, &1).await;

    loop {
        match connection.next().await {
            GattConnectionEvent::Disconnected { reason } => {
                let _ = server.set(&lifecycle, &0);
                esp_println::println!("BLE_PHASE1 state=disconnected reason={:?}", reason);
                return;
            }
            GattConnectionEvent::Gatt { event } => {
                let echo_written =
                    matches!(&event, GattEvent::Write(write) if write.handle() == echo.handle);
                match event.accept() {
                    Ok(reply) => reply.send().await,
                    Err(error) => {
                        esp_println::println!(
                            "BLE_PHASE1 state=gatt_reply_error error={:?}",
                            error
                        );
                    }
                }
                if echo_written {
                    if let Ok(value) = server.get(echo) {
                        let _ = echo.notify(&connection, &value).await;
                    }
                }
            }
            _ => {}
        }
    }
}
