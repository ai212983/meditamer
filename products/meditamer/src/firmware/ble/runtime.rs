use bt_hci::cmd::info::ReadBdAddr;
use core::sync::atomic::Ordering;
use embassy_futures::select::{select, Either};
use embassy_time::Timer;
use esp_radio::ble::{
    begin_hci_callback_shutdown, hci_callback_stats, hci_transport_stats,
    wait_for_hci_callback_quiescence,
};
use trouble_host::prelude::*;

use super::*;

/// Meditamer's BLE epoch. The target-owned radio supervisor retains this
/// value between windows, so the original peripheral token is reborrowed for
/// every controller lifetime instead of being reacquired with `BT::steal()`.
pub struct Phase1Service {
    bluetooth: esp_hal::peripherals::BT<'static>,
    server: &'static DiagnosticServer<'static>,
}

impl Phase1Service {
    pub fn new(bluetooth: esp_hal::peripherals::BT<'static>) -> Self {
        console::println!(
            "BLE_PHASE1S state=armed build_id={} cycles={} packet_mtu={} packet_count={} coex=false",
            BUILD_ID,
            PHASE1S_CYCLES,
            PACKET_MTU,
            PACKET_COUNT,
        );
        let server: &'static DiagnosticServer<'static> = DIAGNOSTIC_SERVER.init(
            DiagnosticServer::new_with_config(GapConfig::Peripheral(PeripheralConfig {
                name: "Meditamer",
                appearance: &appearance::UNKNOWN,
            }))
            .expect("DiagnosticServer's attribute table fits attribute_table_size=32"),
        );
        let build_info = ble::diagnostic::BuildInfo {
            schema_version: 1,
            protocol_version: 1,
            capabilities: ble::diagnostic::Capabilities::ECHO
                .union(ble::diagnostic::Capabilities::LIFECYCLE_STATUS),
            digest_prefix: {
                use sha2::{Digest, Sha256};
                const PREFIX_LEN: usize = ble::capacity::BUILD_INFO_DIGEST_PREFIX_BYTES;
                let digest = Sha256::digest(BUILD_ID.as_bytes());
                let mut hash = [0u8; PREFIX_LEN];
                hash.copy_from_slice(&digest[..PREFIX_LEN]);
                hash
            },
        }
        .encode();
        let _ = server.set(&server.diagnostic.build_info, &build_info);
        Self { bluetooth, server }
    }

    pub async fn run_next(&mut self, _boot_generation: u32, _epoch: u32) {
        let request = PHASE1S_REQUEST.wait().await;
        if PHASE1S_CLOSE_REQUEST.try_take().is_some() || update_reserved() {
            PHASE1D_FAILURE.store(ProbeFailure::UpdateReserved as u8, Ordering::Relaxed);
            PHASE1D_STATE.store(ProbeState::Failed as u8, Ordering::Release);
            publish_ble_ownership();
            return;
        }
        PHASE1D_STATE.store(ProbeState::Running as u8, Ordering::Release);
        publish_ble_ownership();
        let completion = run_phase1s_probe(&mut self.bluetooth, request, self.server).await;
        PHASE1D_FAILURE.store(completion.failure as u8, Ordering::Relaxed);
        PHASE1D_STATE.store(completion.state as u8, Ordering::Release);
        publish_ble_ownership();
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
    bluetooth: &mut esp_hal::peripherals::BT<'static>,
    request: ProbeRequest,
    server: &'static DiagnosticServer<'static>,
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
    if let Err(failure) = run_phase1s_cycle(bluetooth.reborrow(), request, server).await {
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
    device: esp_hal::peripherals::BT<'_>,
    request: ProbeRequest,
    server: &'static DiagnosticServer<'static>,
) -> Result<(), ProbeFailure> {
    let cycle = 1;
    let queue_task_cancelled_before =
        esp_radio::queue_lifecycle_stats().operation_cancelled_on_task_delete;
    if !arbitration::claim::exclusive_lease_matches(request.boot_generation, request.epoch) {
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
    let address = generate_and_record_epoch_address()?;
    let controller: BleController = ExternalController::new(connector);
    let resources = HOST_RESOURCES.initialize(Resources::new());
    let stack = trouble_host::new(controller, resources)
        .set_random_address(address)
        .build();
    let mut peripheral = stack.peripheral();
    let runner = stack.runner();
    let mut lifecycle =
        ble::diagnostic::LifecycleStatusPublisher::new(ble::diagnostic::LifecycleStatus {
            state: ble::diagnostic::LifecycleState::Advertising,
            remaining_seconds: (DEADLINE_CONFIG.advertising_ticks / ONE_SECOND_TICKS) as u16,
            rx_drops: 0,
            tx_timeouts: 0,
        });
    let _ = server.set(
        &server.diagnostic.lifecycle_status,
        &lifecycle.latest().encode(),
    );
    let (host_failure, active_sample) = {
        let mut host = core::pin::pin!(run_host(runner));
        let initialized =
            embassy_time::with_timeout(HOST_INIT_TIMEOUT, stack.command(ReadBdAddr::new()));
        match select(host.as_mut(), initialized).await {
            Either::First(_) => (Some(ProbeFailure::HostExited), SampleResult::failed()),
            Either::Second(Ok(Ok(_))) => {
                let close = super::service::advertise_and_serve_window(
                    &mut peripheral,
                    server,
                    &mut lifecycle,
                    request,
                );
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
    // Controller teardown also closes ingress when host initialization or
    // execution ends without returning through the service window.
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
    drop(stack);
    // The host runner borrow ended above; callback shutdown and quiescence
    // prevent transport access while the reusable slots are cleared.
    unsafe {
        HOST_RESOURCES.clear();
    }
    Timer::after(CALLBACK_QUIET_DWELL).await;
    let teardown_transport = hci_transport_stats();
    let settled = hci_callback_stats();
    if settled.in_flight != 0
        || settled.accepted != quiescent.accepted
        || settled.rejected != quiescent.rejected
        || quiescent.rejected != 0
    {
        return Err(ProbeFailure::LateCallback);
    }
    if Phase1PacketPool::free_count() != PACKET_COUNT {
        return Err(ProbeFailure::PacketLeak);
    }
    let queue_task_cancelled = ble::teardown::validate_queue_teardown(
        esp_radio::queue_lifecycle_stats().into(),
        queue_task_cancelled_before,
    )
    .map_err(|_| ProbeFailure::QueueLifecycle)?;
    PHASE1S_QUEUE_TASK_CANCELLED.store(
        queue_task_cancelled.min(u32::MAX as usize) as u32,
        Ordering::Relaxed,
    );
    ble::teardown::validate_transport_teardown(transport.into(), teardown_transport.into())
        .map_err(|_| ProbeFailure::TransportFault)?;
    if Phase1PacketPool::exhausted_count() != 0 {
        return Err(ProbeFailure::PacketExhausted);
    }
    console::println!(
        "BLE_PHASE1D close cycle={} deadline_ms=2000 pre_in_flight={} accepted={} rejected={} callback_high_water={} settled_in_flight={} rx_queue_high_water={} rx_queue_overflow={} rx_oversize={} tx_rejected={} tx_timeout={} transport_faulted={} packets_free={} pool_exhausted={}",
        cycle, before_drop.in_flight, settled.accepted, settled.rejected, settled.high_water,
        settled.in_flight, transport.rx_queue_high_water, transport.rx_queue_overflow,
        transport.rx_oversize, transport.tx_rejected, transport.tx_timeout, transport.faulted,
        Phase1PacketPool::free_count(), Phase1PacketPool::exhausted_count(),
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

fn cancel_host_parts<T>(_parts: T) {}

async fn run_host(mut runner: Runner<'_, BleController<'_>, Phase1PacketPool>) {
    if let Err(error) = runner.run().await {
        console::println!(
            "BLE_PHASE1 state=host_error error={:?} pool_exhausted={}",
            error,
            Phase1PacketPool::exhausted_count()
        );
    }
}
