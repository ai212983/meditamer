use core::cell::RefCell;
use core::future::Future;
use core::pin::{pin, Pin};

use critical_section::Mutex;
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::peripherals::BT;
use esp_radio::ble::controller::BleConnector;
use static_cell::StaticCell;
use trouble_host::prelude::*;

use super::discovery::{discover_services, inspect_services};
use super::hid_capture::capture_hid_input;
use super::protocol::{
    next_pairing_event, race_runner, record_completed_pairing, record_remote_disconnect,
    NameMatchHandler, PairingEvent, RuntimeRace,
};
use super::{
    Controller, LiveConnectOptions, LiveConnectOutcome, LiveConnectReport, Resources,
    MAX_DISCOVERED_SERVICES,
};
use crate::gatt::{Connection as SessionConnection, GattSession};

pub(super) struct SessionState<'options, 'session> {
    pub(super) options: LiveConnectOptions<'options>,
    pub(super) report: &'session mut LiveConnectReport,
    pub(super) connection: SessionConnection,
    pub(super) bond_table: &'session mut crate::bond::BondTable,
}

impl SessionState<'_, '_> {
    pub(super) fn fail(&mut self, outcome: LiveConnectOutcome) {
        self.report.outcome = outcome;
    }

    pub(super) fn stop_requested(&self) -> bool {
        self.options
            .stop_requested
            .is_some_and(|predicate| predicate())
    }
}

/// Find, connect to, discover, optionally capture HID input, and disconnect.
///
/// Every remote operation after scanning has its own `connect_timeout`.
/// Existing bonds are registered before connection, and a completed new bond
/// is retained only in the caller-owned in-memory table. The caller also owns
/// `report` so the generated async state carries a reference instead of moving
/// the bounded discovery and HID buffers through nested poll frames.
pub async fn run(
    device: BT<'static>,
    options: LiveConnectOptions<'_>,
    bond_table: &mut crate::bond::BondTable,
    report: &mut LiveConnectReport,
) {
    static RESOURCES: StaticCell<Resources> = StaticCell::new();
    let resources = RESOURCES.init(Resources::new());
    let _ = run_with_resources(device, options, bond_table, report, resources).await;
}

/// Run one controller epoch using storage that can be reused after this call.
/// Returns true only after callbacks, controller queues, and transport close cleanly.
/// A false result must prevent another radio owner from taking over or restarting.
pub async fn run_with_resources(
    device: BT<'_>,
    options: LiveConnectOptions<'_>,
    bond_table: &mut crate::bond::BondTable,
    report: &mut LiveConnectReport,
    resources: &mut Resources,
) -> bool {
    use embassy_futures::select::{select, Either};
    use esp_radio::ble::{
        begin_hci_callback_shutdown, hci_callback_stats, hci_transport_stats,
        wait_for_hci_callback_quiescence,
    };
    report.reset();
    let cancelled_before = esp_radio::queue_lifecycle_stats().operation_cancelled_on_task_delete;
    let Some(stack) = initialize_stack(device, resources) else {
        return false;
    };
    {
        let cancelled = async {
            loop {
                if options.stop_requested.is_some_and(|stop| stop()) {
                    return;
                }
                Timer::after_millis(20).await;
            }
        };
        if let Either::First(()) =
            select(cancelled, run_session(&stack, options, bond_table, report)).await
        {
            report.outcome = LiveConnectOutcome::Cancelled;
        }
    }
    // All host futures are gone. Close controller ingress before dropping the
    // stack and its connector; the vendor Drop then deinitializes BTDM and
    // reclaims its queue epoch only after the callback source is disabled.
    begin_hci_callback_shutdown();
    let quiescent = wait_for_hci_callback_quiescence(Duration::from_millis(1000)).await;
    let callbacks = hci_callback_stats();
    let transport = hci_transport_stats();
    drop(stack);
    Timer::after_millis(20).await;
    let settled = hci_callback_stats();
    let packets_released = default_packets_released();
    let closed = packets_released
        && quiescent
        && !settled.admission_open
        && settled.in_flight == 0
        && settled.accepted == callbacks.accepted
        && settled.rejected == callbacks.rejected
        && crate::teardown::validate_queue_teardown(
            esp_radio::queue_lifecycle_stats().into(),
            cancelled_before,
        )
        .is_ok()
        && crate::teardown::validate_transport_teardown(
            transport.into(),
            hci_transport_stats().into(),
        )
        .is_ok();
    if let Some(sink) = options.hid_input_sink {
        sink.input_closed((options.now_ticks)());
    }
    closed
}

// Construct host backing state before entering the deep session poll chain.
// Returning Stack only moves its small controller/reference handles.
#[inline(never)]
fn initialize_stack<'d, 'resources>(
    device: BT<'d>,
    resources: &'resources mut Resources,
) -> Option<Stack<'resources, Controller<'d>, DefaultPacketPool>> {
    let connector = BleConnector::new(device, Default::default()).ok()?;
    Some(
        trouble_host::new(ExternalController::new(connector), resources)
            .set_random_address(Address::random([0xfe, 0x43, 0x4f, 0x4e, 0x4e, 0x01]))
            .build(),
    )
}

// Trouble's default pool is global. Reusing HostResources is allowed only if
// no packet is still retained in an old connection/channel backing slot.
fn default_packets_released() -> bool {
    let mut packets = heapless::Vec::<_, { trouble_host::config::DEFAULT_PACKET_POOL_SIZE }>::new();
    if DefaultPacketPool::capacity() > packets.capacity() {
        return false;
    }
    for _ in 0..DefaultPacketPool::capacity() {
        let Some(packet) = DefaultPacketPool::allocate() else {
            return false;
        };
        if packets.push(packet).is_err() {
            return false;
        }
    }
    true
}

async fn run_session(
    stack: &Stack<'_, Controller<'_>, DefaultPacketPool>,
    options: LiveConnectOptions<'_>,
    bond_table: &mut crate::bond::BondTable,
    report: &mut LiveConnectReport,
) {
    stack.set_io_capabilities(IoCapabilities::NoInputNoOutput);

    let mut state = SessionState {
        options,
        report,
        connection: SessionConnection::new(),
        bond_table,
    };
    if state.stop_requested() {
        state.fail(LiveConnectOutcome::Cancelled);
        return;
    }
    if state.options.request_pairing {
        for record in state.bond_table.iter() {
            let bond = crate::pairing::bond_information_from_record(record);
            let _ = stack.add_bond_information(bond);
        }
    }

    let mut runner = stack.runner();
    let mut central = stack.central();
    let found: Mutex<RefCell<Option<(AddrKind, [u8; 6])>>> = Mutex::new(RefCell::new(None));
    let handler = NameMatchHandler {
        target_name: state.options.target_name,
        found: &found,
    };
    let mut runner_future = pin!(async { runner.run_with_handler(&handler).await });

    let target = scan_target(&mut state, &mut central, &found, runner_future.as_mut()).await;
    let Some(target) = target else {
        return;
    };
    let connection = connect_target(&mut state, &mut central, target, runner_future.as_mut()).await;
    let Some(connection) = connection else {
        return;
    };

    if state.options.request_pairing {
        connection.set_bondable(true).ok();
    }
    let mut paired_this_call = false;
    if state.options.request_pairing && state.options.pair_before_discovery {
        paired_this_call = true;
        if !pair_connection(&mut state, &connection, runner_future.as_mut()).await {
            return;
        }
    }

    let client = initialize_client(&stack, &connection, &mut state, runner_future.as_mut()).await;
    let Some(client) = client else {
        return;
    };
    let mut client_task_future = pin!(async { client.task().await });
    let mut gatt_session = GattSession::new();
    gatt_session.begin_discovery();

    let services = discover_services(
        &mut state,
        &connection,
        &client,
        &mut gatt_session,
        &mut paired_this_call,
        runner_future.as_mut(),
        client_task_future.as_mut(),
    )
    .await;
    let Some(services) = services else {
        return;
    };
    if !inspect_services(
        &mut state,
        &client,
        &mut gatt_session,
        &services,
        runner_future.as_mut(),
        client_task_future.as_mut(),
    )
    .await
    {
        return;
    }
    gatt_session.finish_discovery(true);

    if !capture_hid_input(
        &mut state,
        &connection,
        &client,
        runner_future.as_mut(),
        client_task_future.as_mut(),
    )
    .await
    {
        return;
    }

    disconnect(&mut state, connection);
}

async fn scan_target<R>(
    state: &mut SessionState<'_, '_>,
    central: &mut Central<'_, Controller<'_>, DefaultPacketPool>,
    found: &Mutex<RefCell<Option<(AddrKind, [u8; 6])>>>,
    mut runner: Pin<&mut R>,
) -> Option<(AddrKind, [u8; 6])>
where
    R: Future,
{
    if state.stop_requested() {
        state.fail(LiveConnectOutcome::Cancelled);
        return None;
    }
    let mut scanner = Scanner::new(central);
    let scan_config = ScanConfig {
        active: true,
        ..Default::default()
    };
    const SCAN_POLL_INTERVAL: Duration = Duration::from_millis(200);
    let scan_body = async {
        // The UI may wait for scan readiness. Bound host initialization and
        // scan enable as well as the subsequent device-discovery window.
        match with_timeout(state.options.connect_timeout, scanner.scan(&scan_config)).await {
            Ok(Ok(session)) => {
                if let Some(complete) = state.options.startup_complete {
                    complete();
                }
                let mut waited = Duration::from_millis(0);
                while critical_section::with(|token| found.borrow_ref(token).is_none()) {
                    if state.stop_requested() {
                        state.fail(LiveConnectOutcome::Cancelled);
                        break;
                    }
                    if waited >= state.options.scan_timeout {
                        break;
                    }
                    Timer::after(SCAN_POLL_INTERVAL).await;
                    waited += SCAN_POLL_INTERVAL;
                }
                drop(session);
                Ok(())
            }
            Ok(Err(_)) | Err(_) => Err(()),
        }
    };
    let scan_completed = match race_runner(runner.as_mut(), scan_body).await {
        RuntimeRace::Exited => {
            state.fail(LiveConnectOutcome::HostExited);
            false
        }
        RuntimeRace::Operation(Err(())) => {
            state.fail(LiveConnectOutcome::ScanStartFailed);
            false
        }
        RuntimeRace::Operation(Ok(())) => true,
    };
    if !scan_completed {
        return None;
    }

    let target = critical_section::with(|token| *found.borrow_ref(token));
    if let Some((_, address)) = target {
        state.report.address = Some(address);
        drop(scanner);
        target
    } else {
        state.fail(LiveConnectOutcome::DeviceNotFound);
        None
    }
}

async fn connect_target<'stack, R>(
    state: &mut SessionState<'_, '_>,
    central: &mut Central<'stack, Controller<'_>, DefaultPacketPool>,
    target: (AddrKind, [u8; 6]),
    mut runner: Pin<&mut R>,
) -> Option<Connection<'stack, DefaultPacketPool>>
where
    R: Future,
{
    if state.stop_requested() {
        state.fail(LiveConnectOutcome::Cancelled);
        return None;
    }
    let deadline =
        (state.options.now_ticks)().saturating_add(state.options.connect_timeout.as_micros());
    state
        .connection
        .begin_connect(deadline)
        .expect("a fresh connection can begin connecting");
    let target_address = Address::new(target.0, BdAddr::new(target.1));
    let connect_config = ConnectConfig {
        scan_config: ScanConfig {
            active: true,
            filter_accept_list: &[target_address],
            timeout: state.options.connect_timeout,
            ..Default::default()
        },
        connect_params: RequestedConnParams::default(),
    };
    let operation = with_timeout(
        state.options.connect_timeout,
        central.connect(&connect_config),
    );
    match race_runner(runner.as_mut(), operation).await {
        RuntimeRace::Exited => state.fail(LiveConnectOutcome::HostExited),
        RuntimeRace::Operation(Err(_)) | RuntimeRace::Operation(Ok(Err(_))) => {
            state.connection.fault(state.connection.generation());
            state.report.connection_state = state.connection.state();
            state.fail(LiveConnectOutcome::ConnectFailed);
        }
        RuntimeRace::Operation(Ok(Ok(connection))) => {
            state
                .connection
                .mark_connected(state.connection.generation())
                .expect("begin_connect makes mark_connected legal");
            state.report.connection_state = state.connection.state();
            return Some(connection);
        }
    }
    None
}

async fn initialize_client<'connection, 'd, R>(
    stack: &Stack<'_, Controller<'d>, DefaultPacketPool>,
    connection: &Connection<'connection, DefaultPacketPool>,
    state: &mut SessionState<'_, '_>,
    mut runner: Pin<&mut R>,
) -> Option<GattClient<'connection, Controller<'d>, DefaultPacketPool, MAX_DISCOVERED_SERVICES>>
where
    R: Future,
{
    if state.stop_requested() {
        state.fail(LiveConnectOutcome::Cancelled);
        return None;
    }
    let operation = with_timeout(
        state.options.connect_timeout,
        GattClient::new(stack, connection),
    );
    match race_runner(runner.as_mut(), operation).await {
        RuntimeRace::Exited => state.fail(LiveConnectOutcome::HostExited),
        RuntimeRace::Operation(Err(_)) | RuntimeRace::Operation(Ok(Err(_))) => {
            state.fail(LiveConnectOutcome::GattClientInitFailed);
        }
        RuntimeRace::Operation(Ok(Ok(client))) => return Some(client),
    }
    None
}

pub(super) async fn pair_connection<R>(
    state: &mut SessionState<'_, '_>,
    connection: &Connection<'_, DefaultPacketPool>,
    mut runner: Pin<&mut R>,
) -> bool
where
    R: Future,
{
    if state.stop_requested() {
        state.fail(LiveConnectOutcome::Cancelled);
        return false;
    }
    connection.set_bondable(true).ok();
    if connection.request_security().is_err() {
        state.fail(LiveConnectOutcome::SecurityRequestFailed);
        return false;
    }
    let started_at = (state.options.now_ticks)();
    let pairing = with_timeout(
        state.options.connect_timeout,
        next_pairing_event(connection),
    );
    let event = match race_runner(runner.as_mut(), pairing).await {
        RuntimeRace::Exited => {
            state.fail(LiveConnectOutcome::HostExited);
            return false;
        }
        RuntimeRace::Operation(Err(_)) => {
            record_pairing_elapsed(state, started_at);
            state.fail(LiveConnectOutcome::PairingTimedOut);
            return false;
        }
        RuntimeRace::Operation(Ok(event)) => event,
    };
    record_pairing_elapsed(state, started_at);
    match event {
        PairingEvent::Failed => state.fail(LiveConnectOutcome::PairingFailed),
        PairingEvent::Disconnected(reason) => {
            record_remote_disconnect(
                &mut state.report,
                &mut state.connection,
                (state.options.now_ticks)(),
                reason,
            );
            state.fail(LiveConnectOutcome::DisconnectedDuringPairing);
        }
        PairingEvent::Complete {
            security_level,
            bond,
        } => {
            record_completed_pairing(&mut state.report, state.bond_table, security_level, bond);
            return true;
        }
    }
    false
}

fn record_pairing_elapsed(state: &mut SessionState<'_, '_>, started_at: u64) {
    state.report.pairing_elapsed_us = Some((state.options.now_ticks)().saturating_sub(started_at));
}

fn disconnect(state: &mut SessionState<'_, '_>, connection: Connection<'_, DefaultPacketPool>) {
    let generation = state.connection.generation();
    let deadline =
        (state.options.now_ticks)().saturating_add(state.options.connect_timeout.as_micros());
    state.connection.begin_disconnect(generation, deadline).ok();
    drop(connection);
    state.connection.mark_disconnected(generation).ok();
    state.report.connection_state = state.connection.state();
    state.report.outcome = LiveConnectOutcome::Completed;
}
