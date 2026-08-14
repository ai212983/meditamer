//! Restartable owner for Wi-Fi, Embassy networking, and HTTP service epochs.

mod off_resources;

#[cfg(feature = "ble-foundation")]
use core::sync::atomic::AtomicU32;
use core::{
    future::Future,
    pin::pin,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
};

use embassy_futures::{
    join::join3,
    select::{select, select3, Either, Either3},
};
use embassy_net::{Runner, StackResources};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::rng::Rng;
use esp_println::println;
use static_cell::StaticCell;

use super::handoff::control_quiescence_complete;
#[cfg(feature = "ble-foundation")]
use super::handoff::request_matches_ack;
use super::wifi::{WifiController, WifiDevice};
use super::{
    handoff::{
        classify_drain, sd_barrier_complete, DrainAction, NetworkOwnerAck, NetworkOwnerAckKind,
        NetworkOwnerCommand, NetworkOwnerMachine, NetworkOwnerRequest, OwnerAction, RejectReason,
        ResourceSnapshot, TeardownSequence, TeardownStage,
    },
    wifi,
};
use crate::firmware::{
    observability as telemetry, psram, service_mode, storage, types::WifiRuntimePolicy,
    update as firmware_update,
};
use off_resources::settled_off_resource_snapshot;

const NET_STACK_SOCKETS: usize = 4;
const ACTIVE_OPERATION_GRACE: Duration = Duration::from_secs(12);
const FORCED_ABORT_GRACE: Duration = Duration::from_secs(2);
const WIFI_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE_QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(2);
const QUIESCENCE_POLL: Duration = Duration::from_millis(50);
const RESTORE_MIN_TIMEOUT_MS: u64 = 15_000;
const RESTORE_MAX_TIMEOUT_MS: u64 = 180_000;
const INTERNAL_RESERVE_BYTES: usize = 16_384;
const REQUIRED_BLE_ALLOCATION_BYTES: usize = 4_112;
const REQUIRED_OFF_FREE_BYTES: usize = INTERNAL_RESERVE_BYTES + REQUIRED_BLE_ALLOCATION_BYTES;

static STACK_RESOURCES: StaticCell<StackResources<NET_STACK_SOCKETS>> = StaticCell::new();
static HANDOFF_COMMANDS: Channel<CriticalSectionRawMutex, NetworkOwnerRequest, 2> = Channel::new();
static HANDOFF_ACKS: Channel<CriticalSectionRawMutex, NetworkOwnerAck, 2> = Channel::new();
#[cfg(feature = "ble-foundation")]
static HANDOFF_REQUEST_ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "ble-foundation")]
static NEXT_HANDOFF_REQUEST_ID: AtomicU32 = AtomicU32::new(1);
static NETWORK_OWNER_RESIDENT: AtomicBool = AtomicBool::new(false);
static WIFI_CONTROLLER_RESIDENT: AtomicBool = AtomicBool::new(false);
static NET_RUNNER_RESIDENT: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "ble-foundation")]
static OFF_LEASE_VALID: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "ble-foundation")]
static OFF_LEASE_BOOT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "ble-foundation")]
static OFF_LEASE_EPOCH: AtomicU32 = AtomicU32::new(0);

#[cfg(feature = "ble-foundation")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NetRuntimeResidency {
    pub(crate) wifi_controller_task: bool,
    pub(crate) net_runner_task: bool,
}

#[cfg(feature = "ble-foundation")]
pub(crate) fn residency_snapshot() -> NetRuntimeResidency {
    NetRuntimeResidency {
        wifi_controller_task: WIFI_CONTROLLER_RESIDENT.load(Ordering::Acquire),
        net_runner_task: NET_RUNNER_RESIDENT.load(Ordering::Acquire),
    }
}

#[cfg(feature = "ble-foundation")]
pub(crate) fn exclusive_lease_matches(boot_generation: u32, epoch: u32) -> bool {
    OFF_LEASE_VALID.load(Ordering::Acquire)
        && OFF_LEASE_BOOT.load(Ordering::Relaxed) == boot_generation
        && OFF_LEASE_EPOCH.load(Ordering::Relaxed) == epoch
}

#[cfg(feature = "ble-foundation")]
fn publish_exclusive_lease(boot_generation: u32, epoch: u32) {
    OFF_LEASE_BOOT.store(boot_generation, Ordering::Relaxed);
    OFF_LEASE_EPOCH.store(epoch, Ordering::Relaxed);
    OFF_LEASE_VALID.store(true, Ordering::Release);
}

#[cfg(feature = "ble-foundation")]
fn clear_exclusive_lease() {
    OFF_LEASE_VALID.store(false, Ordering::Release);
}

pub(crate) fn stack_resources() -> &'static mut StackResources<NET_STACK_SOCKETS> {
    STACK_RESOURCES.init(StackResources::new())
}

#[cfg(feature = "ble-foundation")]
#[derive(Clone, Copy)]
pub(crate) struct HandoffTicket(u32);

#[cfg(feature = "ble-foundation")]
pub(crate) fn request_handoff(command: NetworkOwnerCommand) -> Result<HandoffTicket, ()> {
    HANDOFF_REQUEST_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| ())?;
    let request_id = next_handoff_request_id();
    if HANDOFF_COMMANDS
        .try_send(NetworkOwnerRequest {
            request_id,
            command,
        })
        .is_err()
    {
        HANDOFF_REQUEST_ACTIVE.store(false, Ordering::Release);
        return Err(());
    }
    Ok(HandoffTicket(request_id))
}

#[cfg(feature = "ble-foundation")]
fn next_handoff_request_id() -> u32 {
    loop {
        let request_id = NEXT_HANDOFF_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        if request_id != 0 {
            return request_id;
        }
    }
}

#[cfg(feature = "ble-foundation")]
pub(crate) async fn receive_handoff_ack(ticket: HandoffTicket) -> NetworkOwnerAck {
    loop {
        let ack = HANDOFF_ACKS.receive().await;
        if request_matches_ack(ticket.0, &ack) {
            HANDOFF_REQUEST_ACTIVE.store(false, Ordering::Release);
            return ack;
        }
    }
}

#[cfg(feature = "ble-foundation")]
pub(crate) fn cancel_handoff_request(_ticket: HandoffTicket) {
    HANDOFF_REQUEST_ACTIVE.store(false, Ordering::Release);
}

fn publish_ack(ack: NetworkOwnerAck) {
    // Request ID zero is reserved for unsolicited startup/retry state. It is
    // logged separately but must never occupy the bounded reply channel.
    if ack.request_id == 0 {
        return;
    }
    if HANDOFF_ACKS.try_send(ack).is_err() {
        println!(
            "RADIO_HANDOFF state=fault reason=ack_queue_full request={} boot={} epoch={}",
            ack.request_id, ack.boot_generation, ack.epoch
        );
    }
}

struct ResidencyGuard(&'static AtomicBool);

impl ResidencyGuard {
    fn enter(slot: &'static AtomicBool) -> Self {
        assert!(
            !slot.swap(true, Ordering::AcqRel),
            "network owner started twice"
        );
        Self(slot)
    }
}

impl Drop for ResidencyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy)]
enum EpochResult {
    Quiesced { epoch: u32 },
}

enum DrainResult {
    Cancelled,
    Complete,
}

#[embassy_executor::task]
pub(crate) async fn network_owner_task(
    mut wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
    resources: &'static mut StackResources<NET_STACK_SOCKETS>,
) {
    #[cfg(feature = "ble-foundation")]
    clear_exclusive_lease();
    let _owner = ResidencyGuard::enter(&NETWORK_OWNER_RESIDENT);
    let rng = Rng::new();
    let mut boot_generation = rng.random();
    if boot_generation == 0 {
        boot_generation = 1;
    }
    let mut machine = NetworkOwnerMachine::new(boot_generation);
    let mut restore_epoch = 0;
    let mut pending_rejection: Option<(u32, RejectReason)> = None;

    println!(
        "RADIO_HANDOFF state=restoring boot={} epoch=0",
        machine.boot_generation()
    );

    loop {
        service_mode::set_radio_handoff_admission_open(true);
        let epoch_result = run_network_epoch(
            &mut wifi_peripheral,
            resources,
            &mut machine,
            restore_epoch,
            &mut pending_rejection,
        )
        .await;
        match epoch_result {
            Ok(EpochResult::Quiesced { epoch }) => {
                // The completed epoch future must be gone before the binding
                // allocation probe. ESP-RTOS may defer freeing a self-deleting
                // Wi-Fi task until a later scheduler pass, so also require a
                // bounded, coherent allocator settlement window.
                let resources = settled_off_resource_snapshot().await;
                let floor_ok = resources.stable
                    && resources.internal_free_bytes as usize >= REQUIRED_OFF_FREE_BYTES
                    && resources.largest_block_above_reserve_bytes as usize
                        >= REQUIRED_BLE_ALLOCATION_BYTES
                    && resources.probe_free_before_bytes as usize >= REQUIRED_OFF_FREE_BYTES
                    && resources.probe_free_after_bytes as usize >= REQUIRED_OFF_FREE_BYTES
                    && resources.probe_reserve_bytes as usize == INTERNAL_RESERVE_BYTES
                    && resources.http_connections == 0
                    && resources.sd_roundtrips == 0
                    && resources.sd_sessions == 0
                    && resources.radio_callbacks == 0
                    && resources.radio_queues == 0
                    && !resources.radio_source_active
                    && !resources.callback_admission_open
                    && resources.late_callbacks == 0
                    && resources.queue_late_use == 0
                    && resources.queue_unknown_use == 0
                    && resources.queue_reclaim_failures == 0
                    && resources.queue_corruption == 0
                    && resources.queue_contention == 0;
                if !floor_ok {
                    machine.begin_rollback();
                    pending_rejection = Some((epoch, RejectReason::ResourceFloor));
                    restore_epoch = epoch;
                    println!(
                        "RADIO_HANDOFF state=restoring boot={} epoch={} reason=resource_floor internal_free={} block_above_reserve={} stable={}",
                        machine.boot_generation(),
                        epoch,
                        resources.internal_free_bytes,
                        resources.largest_block_above_reserve_bytes,
                        resources.stable,
                    );
                    continue;
                }

                let ack = machine.quiesced(epoch, resources);
                #[cfg(feature = "ble-foundation")]
                publish_exclusive_lease(machine.boot_generation(), epoch);
                publish_ack(ack);
                println_ack(ack);
                restore_epoch = wait_while_off(&mut machine).await;
            }
            Err(reason) => {
                let resources = resource_snapshot(false);
                let ack = machine.restore_failed(restore_epoch, resources);
                println!(
                    "RADIO_HANDOFF state=restoring boot={} epoch={} reason={} setup={}",
                    machine.boot_generation(),
                    restore_epoch,
                    RejectReason::RestoreFailed.label(),
                    reason,
                );
                service_restore_wait(&mut machine, ack).await;
                continue;
            }
        }
    }
}

async fn run_network_epoch(
    wifi_peripheral: &mut esp_hal::peripherals::WIFI<'static>,
    resources: &mut StackResources<NET_STACK_SOCKETS>,
    machine: &mut NetworkOwnerMachine,
    restore_epoch: u32,
    pending_rejection: &mut Option<(u32, RejectReason)>,
) -> Result<EpochResult, &'static str> {
    println!("upload_http: wifi_backend name={}", wifi::backend_name());
    let (mut controller, sta_device) =
        match wifi::initialize_runtime_sta(wifi_peripheral.reborrow()) {
            Ok(parts) => parts,
            Err("asset-upload-http: wifi ownership unknown") => {
                fault_holding_peripheral(machine, restore_epoch, RejectReason::OwnershipUnknown)
                    .await
            }
            Err(reason) => return Err(reason),
        };
    let _controller_resident = ResidencyGuard::enter(&WIFI_CONTROLLER_RESIDENT);
    wifi::apply_runtime_setup_overrides_and_log();

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let (stack, mut runner) = embassy_net::new(
        sta_device,
        embassy_net::Config::dhcpv4(Default::default()),
        resources,
        seed,
    );
    let _runner_resident = ResidencyGuard::enter(&NET_RUNNER_RESIDENT);
    telemetry::set_wifi_link_connected(false);
    telemetry::set_upload_http_listener(false, None);

    let mut current_restore_epoch = restore_epoch;
    let (epoch, service_error) = loop {
        // Re-read live config on every iteration, not only once at epoch entry.
        // A retry within this epoch (finish_product_quiescence() returning false)
        // recreates the connection task below; reusing a config snapshot captured
        // before this iteration started would silently discard credentials/policy
        // updates the previous connection attempt had already picked up live (see
        // docs/plans/ble-phase1s-capacity-recovery-ledger.md CAP-0006).
        let runtime_config = wifi::current_runtime_config();
        let service_result = {
            let connection = wifi::run_wifi_connection_task(
                &mut controller,
                runtime_config.credentials,
                runtime_config.policy,
                stack,
            );
            let network = run_network_runner(&mut runner);
            let http = storage::upload::run_http_server(stack);
            let mut services = pin!(join3(connection, network, http));
            let restored = await_restoration(
                services.as_mut(),
                machine,
                current_restore_epoch,
                pending_rejection,
                runtime_config.policy,
            )
            .await;
            let result = match restored {
                Ok(()) => await_exclusive_request(services.as_mut(), machine).await,
                Err(reason) => Err(reason),
            };
            if let Err(reason) = result {
                if reason.contains("services returned") {
                    fault_holding_services(
                        machine,
                        current_restore_epoch,
                        RejectReason::OwnershipUnknown,
                        services.as_mut(),
                    )
                    .await;
                }
                service_mode::set_radio_handoff_admission_open(false);
                wifi::request_control_quiescence();
                if !wait_for_control_quiescence(services.as_mut()).await {
                    fault_holding_services(
                        machine,
                        current_restore_epoch,
                        RejectReason::OwnershipUnknown,
                        services.as_mut(),
                    )
                    .await;
                }
            }
            result
        };
        wifi::cancel_control_quiescence();

        // All service futures have now been dropped. That closes the socket and
        // releases any cancelled SD roundtrip lock before the correlated Abort.
        let (epoch, service_error) = match service_result {
            Ok(epoch) => (epoch, None),
            Err(reason) => (current_restore_epoch, Some(reason)),
        };
        if finish_product_quiescence().await {
            break (epoch, service_error);
        }
        if service_error.is_some() {
            fault_holding_owners(machine, epoch, RejectReason::OwnershipUnknown, controller).await;
        }

        // Ownership is still known and the runner/controller remain alive.
        // Recreate only the local service futures, restore admission, and reject
        // the lease after readiness is re-established.
        machine.begin_rollback();
        *pending_rejection = Some((epoch, RejectReason::QuiescenceTimeout));
        current_restore_epoch = epoch;
        telemetry::set_wifi_link_connected(false);
        telemetry::set_upload_http_listener(false, None);
        service_mode::set_radio_handoff_admission_open(true);
    };

    let mut teardown = TeardownSequence::new();
    teardown
        .advance(TeardownStage::ProductQuiesced)
        .expect("teardown product stage");

    telemetry::set_wifi_link_connected(false);
    telemetry::set_upload_http_listener(false, None);
    drop(runner);
    teardown
        .advance(TeardownStage::RunnerDropped)
        .expect("teardown runner stage");
    if !matches!(
        with_timeout(WIFI_STOP_TIMEOUT, wifi::wifi_stop_async(&mut controller)).await,
        Ok(Ok(()))
    ) {
        fault_holding_owners(machine, epoch, RejectReason::OwnershipUnknown, controller).await;
    }
    teardown
        .advance(TeardownStage::WifiStopped)
        .expect("teardown stop stage");

    if wifi::wifi_shutdown_source(&mut controller).is_err() {
        fault_holding_owners(machine, epoch, RejectReason::OwnershipUnknown, controller).await;
    }
    teardown
        .advance(TeardownStage::SourceDisabled)
        .expect("teardown source stage");

    let source_quiescent = with_timeout(SOURCE_QUIESCENCE_TIMEOUT, async {
        loop {
            let callbacks = wifi::wifi_callback_stats();
            if callbacks.in_flight == 0
                && callbacks.tx_in_flight == 0
                && !callbacks.rx_sta_callback_owned
                && esp_radio::queue_lifecycle_stats().in_flight == 0
            {
                break;
            }
            Timer::after(QUIESCENCE_POLL).await;
        }
    })
    .await
    .is_ok();
    if !source_quiescent {
        fault_holding_owners(machine, epoch, RejectReason::OwnershipUnknown, controller).await;
    }
    teardown
        .advance(TeardownStage::CallbacksQuiesced)
        .expect("teardown callback stage");

    if wifi::wifi_finalize_shutdown(&mut controller).is_err() {
        fault_holding_owners(machine, epoch, RejectReason::OwnershipUnknown, controller).await;
    }
    teardown
        .advance(TeardownStage::QueuesReclaimed)
        .expect("teardown queue stage");

    // Explicit finalization disabled the callback source, released the radio
    // guard, and reclaimed the source-scoped queue epoch. Drop is now a no-op.
    drop(controller);
    resources.reset();
    teardown
        .advance(TeardownStage::StackReset)
        .expect("teardown stack stage");
    assert!(teardown.complete(), "network teardown sequence incomplete");
    if let Some(reason) = service_error {
        return Err(reason);
    }
    Ok(EpochResult::Quiesced { epoch })
}

async fn await_restoration<F: Future>(
    mut services: Pin<&mut F>,
    machine: &mut NetworkOwnerMachine,
    restore_epoch: u32,
    pending_rejection: &mut Option<(u32, RejectReason)>,
    policy: WifiRuntimePolicy,
) -> Result<(), &'static str> {
    let started = embassy_time::Instant::now();
    let timeout = restoration_timeout(policy);
    loop {
        if restoration_ready() {
            let resources = resource_snapshot(false);
            let ack = if let Some((epoch, reason)) = pending_rejection.take() {
                machine.reject_quiesce(epoch, reason, resources)
            } else {
                machine.restored(restore_epoch, resources)
            };
            publish_ack(ack);
            println_ack(ack);
            return Ok(());
        }
        if started.elapsed() >= timeout {
            return Err("restore readiness deadline");
        }
        match select3(
            services.as_mut(),
            HANDOFF_COMMANDS.receive(),
            Timer::after(QUIESCENCE_POLL),
        )
        .await
        {
            Either3::First(_) => return Err("network services returned during restore"),
            Either3::Second(request) => {
                let OwnerAction::None(ack) =
                    machine.command_with_id(request.request_id, request.command)
                else {
                    unreachable!("restoring owner cannot transfer ownership")
                };
                publish_ack(ack);
            }
            Either3::Third(_) => {}
        }
    }
}

async fn await_exclusive_request<F: Future>(
    mut services: Pin<&mut F>,
    machine: &mut NetworkOwnerMachine,
) -> Result<u32, &'static str> {
    loop {
        match select(services.as_mut(), HANDOFF_COMMANDS.receive()).await {
            Either::First(_) => return Err("network services returned"),
            Either::Second(request) => {
                if matches!(
                    request.command,
                    NetworkOwnerCommand::AcquireExclusive { .. }
                ) && update_reserved()
                {
                    publish_ack(reject_without_transition(
                        machine,
                        request.request_id,
                        request.command.epoch(),
                        RejectReason::UpdateReserved,
                    ));
                    continue;
                }
                match machine.command_with_id(request.request_id, request.command) {
                    OwnerAction::None(ack) => publish_ack(ack),
                    OwnerAction::BeginQuiesce { epoch } => {
                        service_mode::set_radio_handoff_admission_open(false);
                        wifi::request_control_quiescence();
                        match drain_product_work(services.as_mut(), machine, epoch).await? {
                            DrainResult::Cancelled => continue,
                            DrainResult::Complete => return Ok(epoch),
                        }
                    }
                    OwnerAction::BeginRestore { .. } => unreachable!(),
                }
            }
        }
    }
}

async fn drain_product_work<F: Future>(
    mut services: Pin<&mut F>,
    machine: &mut NetworkOwnerMachine,
    epoch: u32,
) -> Result<DrainResult, &'static str> {
    let started = embassy_time::Instant::now();
    loop {
        let control_quiescent = network_control_quiescent();
        match classify_drain(
            update_reserved(),
            product_work_quiescent() && control_quiescent,
            started.elapsed() >= ACTIVE_OPERATION_GRACE,
        ) {
            DrainAction::RejectForUpdate => {
                reject_active_quiescence(machine, epoch, RejectReason::UpdateReserved);
                return Ok(DrainResult::Cancelled);
            }
            DrainAction::DropServices => {
                return Ok(DrainResult::Complete);
            }
            DrainAction::ForceAbort if control_quiescent => {
                return Ok(DrainResult::Complete);
            }
            DrainAction::ForceAbort => {
                reject_active_quiescence(machine, epoch, RejectReason::QuiescenceTimeout);
                return Ok(DrainResult::Cancelled);
            }
            DrainAction::Wait => {}
        }
        match select3(
            services.as_mut(),
            HANDOFF_COMMANDS.receive(),
            Timer::after(QUIESCENCE_POLL),
        )
        .await
        {
            Either3::First(_) => return Err("network services returned during quiescence"),
            Either3::Second(request) => {
                match machine.command_with_id(request.request_id, request.command) {
                    OwnerAction::None(ack) => publish_ack(ack),
                    OwnerAction::BeginQuiesce { .. } | OwnerAction::BeginRestore { .. } => {
                        unreachable!()
                    }
                }
            }
            Either3::Third(_) => {}
        }
    }
}

async fn wait_for_control_quiescence<F: Future>(mut services: Pin<&mut F>) -> bool {
    with_timeout(ACTIVE_OPERATION_GRACE, async {
        loop {
            if wifi::control_quiesced() {
                break;
            }
            match select(services.as_mut(), Timer::after(QUIESCENCE_POLL)).await {
                Either::First(_) => core::future::pending::<()>().await,
                Either::Second(_) => {}
            }
        }
    })
    .await
    .is_ok()
}

async fn fault_holding_services<F: Future>(
    machine: &mut NetworkOwnerMachine,
    epoch: u32,
    reason: RejectReason,
    _services: Pin<&mut F>,
) -> ! {
    let ack = machine.fault(epoch, reason, resource_snapshot(false));
    publish_ack(ack);
    println_ack(ack);
    loop {
        let request = HANDOFF_COMMANDS.receive().await;
        let OwnerAction::None(ack) = machine.command_with_id(request.request_id, request.command)
        else {
            unreachable!()
        };
        publish_ack(ack);
    }
}

fn reject_active_quiescence(machine: &mut NetworkOwnerMachine, epoch: u32, reason: RejectReason) {
    wifi::cancel_control_quiescence();
    service_mode::set_radio_handoff_admission_open(true);
    let ack = machine.reject_quiesce(epoch, reason, resource_snapshot(false));
    publish_ack(ack);
}

async fn finish_product_quiescence() -> bool {
    // Abort is also the SD-task FIFO fence. Even when the caller-side roundtrip
    // count and session bit are clear, an earlier timed-out request can still be
    // executing in the single SD owner. Its correlated Abort response proves all
    // earlier work completed before radio ownership is released.
    let started = embassy_time::Instant::now();
    let abort_ok = with_timeout(FORCED_ABORT_GRACE, storage::upload::abort_sd_upload())
        .await
        .unwrap_or(false);
    let elapsed = started.elapsed();
    let remaining = if elapsed < FORCED_ABORT_GRACE {
        FORCED_ABORT_GRACE - elapsed
    } else {
        Duration::from_millis(0)
    };
    let quiescent = if product_work_quiescent() {
        true
    } else {
        with_timeout(remaining, async {
            while !product_work_quiescent() {
                Timer::after(QUIESCENCE_POLL).await;
            }
        })
        .await
        .is_ok()
    };
    sd_barrier_complete(
        abort_ok,
        storage::upload::sd_upload_session_active(),
        quiescent,
    )
}

fn restoration_ready() -> bool {
    let status = wifi::net_status_snapshot();
    wifi_service_ready(&status)
        && (wifi_intentionally_dormant(&status) || !status.listener_enabled || status.listener)
}

fn wifi_service_ready(status: &wifi::NetStatusSnapshot) -> bool {
    (status.state == "Ready" && status.link && status.ipv4 != [0, 0, 0, 0])
        || wifi_intentionally_dormant(status)
}

fn wifi_intentionally_dormant(status: &wifi::NetStatusSnapshot) -> bool {
    !service_mode::upload_enabled()
        && status.state == "Idle"
        && status.radio_quiesced
        && !status.link
}

fn restoration_timeout(policy: WifiRuntimePolicy) -> Duration {
    let dhcp_ms = policy.dhcp_timeout_ms.max(policy.pinned_dhcp_timeout_ms) as u64;
    let total_ms = (policy.connect_timeout_ms as u64)
        .saturating_add(dhcp_ms)
        .saturating_add(policy.listener_timeout_ms as u64)
        .clamp(RESTORE_MIN_TIMEOUT_MS, RESTORE_MAX_TIMEOUT_MS);
    Duration::from_millis(total_ms)
}

async fn run_network_runner(runner: &mut Runner<'_, WifiDevice>) {
    runner.run().await
}

async fn wait_while_off(machine: &mut NetworkOwnerMachine) -> u32 {
    loop {
        let request = HANDOFF_COMMANDS.receive().await;
        #[cfg(feature = "ble-foundation")]
        if matches!(
            request.command,
            NetworkOwnerCommand::ReleaseExclusive { .. }
        ) {
            match crate::firmware::ble::phase1s_ownership() {
                crate::firmware::ble::Phase1sOwnership::Unknown => {
                    clear_exclusive_lease();
                    let ack = machine.fault_for_request(
                        request.request_id,
                        request.command.epoch(),
                        RejectReason::OwnershipUnknown,
                        resource_snapshot(false),
                    );
                    publish_ack(ack);
                    println_ack(ack);
                    loop {
                        let request = HANDOFF_COMMANDS.receive().await;
                        let OwnerAction::None(ack) =
                            machine.command_with_id(request.request_id, request.command)
                        else {
                            unreachable!()
                        };
                        publish_ack(ack);
                    }
                }
                crate::firmware::ble::Phase1sOwnership::Active => {
                    let ack = reject_without_transition(
                        machine,
                        request.request_id,
                        request.command.epoch(),
                        RejectReason::Busy,
                    );
                    publish_ack(ack);
                    continue;
                }
                crate::firmware::ble::Phase1sOwnership::KnownClosed => {}
            }
        }
        match machine.command_with_id(request.request_id, request.command) {
            // OffConfirmed status reports the coherent snapshot that granted
            // the lease. Re-probing here would reopen the deferred-free race
            // after the binding measurement has already passed.
            OwnerAction::None(ack) => publish_ack(ack),
            OwnerAction::BeginRestore { epoch } => {
                #[cfg(feature = "ble-foundation")]
                clear_exclusive_lease();
                return epoch;
            }
            OwnerAction::BeginQuiesce { .. } => unreachable!(),
        }
    }
}

async fn service_restore_wait(machine: &mut NetworkOwnerMachine, ack: NetworkOwnerAck) {
    publish_ack(ack);
    match select(
        HANDOFF_COMMANDS.receive(),
        Timer::after(Duration::from_secs(2)),
    )
    .await
    {
        Either::First(request) => {
            let OwnerAction::None(ack) =
                machine.command_with_id(request.request_id, request.command)
            else {
                return;
            };
            publish_ack(ack);
        }
        Either::Second(_) => {}
    }
}

async fn fault_holding_owners(
    machine: &mut NetworkOwnerMachine,
    epoch: u32,
    reason: RejectReason,
    controller: WifiController<'_>,
) -> ! {
    let _controller = controller;
    let ack = machine.fault(epoch, reason, resource_snapshot(false));
    publish_ack(ack);
    println_ack(ack);
    loop {
        let request = HANDOFF_COMMANDS.receive().await;
        let OwnerAction::None(ack) = machine.command_with_id(request.request_id, request.command)
        else {
            unreachable!()
        };
        publish_ack(ack);
    }
}

async fn fault_holding_peripheral(
    machine: &mut NetworkOwnerMachine,
    epoch: u32,
    reason: RejectReason,
) -> ! {
    let ack = machine.fault(epoch, reason, resource_snapshot(false));
    publish_ack(ack);
    println_ack(ack);
    loop {
        let request = HANDOFF_COMMANDS.receive().await;
        let OwnerAction::None(ack) = machine.command_with_id(request.request_id, request.command)
        else {
            unreachable!()
        };
        publish_ack(ack);
    }
}

fn reject_without_transition(
    machine: &mut NetworkOwnerMachine,
    request_id: u32,
    epoch: u32,
    reason: RejectReason,
) -> NetworkOwnerAck {
    NetworkOwnerAck {
        request_id,
        boot_generation: machine.boot_generation(),
        epoch,
        state: machine.state(),
        kind: NetworkOwnerAckKind::Rejected(reason),
        resources: resource_snapshot(false),
    }
}

fn update_reserved() -> bool {
    firmware_update::transport_quiet()
        || firmware_update::status()
            .map(|status| status.phase != firmware_update::SessionPhase::Idle)
            .unwrap_or(true)
}

fn product_work_quiescent() -> bool {
    storage::upload::active_http_connections() == 0
        && storage::upload::active_sd_roundtrips() == 0
        && !storage::upload::sd_upload_session_active()
}

fn network_control_quiescent() -> bool {
    // Admission shutdown intentionally clears listener/IP telemetry before the
    // controller is torn down. The connection future's explicit held safe-point
    // acknowledgement is the ownership proof; serving-state telemetry is not.
    control_quiescence_complete(wifi::control_quiesced())
}

fn resource_snapshot(probe_largest_block: bool) -> ResourceSnapshot {
    let memory = psram::allocator_memory_snapshot();
    let block = if probe_largest_block {
        psram::probe_internal_block_above_reserve(INTERNAL_RESERVE_BYTES)
    } else {
        crate::firmware::psram::InternalBlockProbe {
            free_before_bytes: memory.free_internal_bytes,
            block_bytes: 0,
            reserve_bytes: INTERNAL_RESERVE_BYTES,
            free_after_bytes: memory.free_internal_bytes,
            stable: true,
        }
    };
    let queues = esp_radio::queue_lifecycle_stats();
    let callbacks = wifi::wifi_callback_stats();
    ResourceSnapshot {
        internal_free_bytes: if probe_largest_block {
            block.free_before_bytes
        } else {
            memory.free_internal_bytes
        }
        .min(u32::MAX as usize) as u32,
        largest_block_above_reserve_bytes: block.block_bytes.min(u32::MAX as usize) as u32,
        probe_free_before_bytes: block.free_before_bytes.min(u32::MAX as usize) as u32,
        probe_free_after_bytes: block.free_after_bytes.min(u32::MAX as usize) as u32,
        probe_reserve_bytes: block.reserve_bytes.min(u32::MAX as usize) as u32,
        http_connections: storage::upload::active_http_connections(),
        sd_roundtrips: storage::upload::active_sd_roundtrips(),
        sd_sessions: u16::from(storage::upload::sd_upload_session_active()),
        radio_callbacks: callbacks.in_flight.min(u16::MAX as u32) as u16,
        radio_queues: queues
            .active
            .saturating_add(queues.retired)
            .saturating_add(queues.owner_active)
            .saturating_add(queues.owner_retired)
            .min(u16::MAX as usize) as u16,
        radio_source_active: callbacks.source_active,
        callback_admission_open: callbacks.admission_open,
        late_callbacks: callbacks.late,
        queue_late_use: queues.late_use_rejected.min(u32::MAX as usize) as u32,
        queue_unknown_use: queues.unknown_use_rejected.min(u32::MAX as usize) as u32,
        queue_reclaim_failures: queues.reclaim_failures.min(u32::MAX as usize) as u32,
        queue_corruption: queues.owner_corruption.min(u32::MAX as usize) as u32,
        queue_contention: queues
            .owner_task_contention_rejected
            .saturating_add(queues.owner_isr_contention_rejected)
            .min(u32::MAX as usize) as u32,
        stable: block.stable,
    }
}

fn println_ack(ack: NetworkOwnerAck) {
    let (kind, reason) = match ack.kind {
        NetworkOwnerAckKind::Status => ("status", "none"),
        NetworkOwnerAckKind::Quiesced => ("quiesced", "none"),
        NetworkOwnerAckKind::Restored => ("restored", "none"),
        NetworkOwnerAckKind::Rejected(reason) => ("rejected", reason.label()),
        NetworkOwnerAckKind::Faulted(reason) => ("faulted", reason.label()),
    };
    println!(
        "RADIO_HANDOFF state={} kind={} reason={} boot={} epoch={} internal_free={} block_above_reserve={} probe_before={} probe_after={} probe_reserve={} http={} sd_roundtrip={} sd_session={} callbacks={} queues={} source_active={} callback_admission={} late_callbacks={} queue_late={} queue_unknown={} queue_reclaim_fail={} queue_corruption={} queue_contention={} stable={}",
        ack.state.label(),
        kind,
        reason,
        ack.boot_generation,
        ack.epoch,
        ack.resources.internal_free_bytes,
        ack.resources.largest_block_above_reserve_bytes,
        ack.resources.probe_free_before_bytes,
        ack.resources.probe_free_after_bytes,
        ack.resources.probe_reserve_bytes,
        ack.resources.http_connections,
        ack.resources.sd_roundtrips,
        ack.resources.sd_sessions,
        ack.resources.radio_callbacks,
        ack.resources.radio_queues,
        ack.resources.radio_source_active,
        ack.resources.callback_admission_open,
        ack.resources.late_callbacks,
        ack.resources.queue_late_use,
        ack.resources.queue_unknown_use,
        ack.resources.queue_reclaim_failures,
        ack.resources.queue_corruption,
        ack.resources.queue_contention,
        ack.resources.stable,
    );
}
