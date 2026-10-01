use core::fmt::Write;

#[cfg(feature = "asset-upload-http")]
use super::io::{run_netcfg_get_command, run_netcfg_set_command};
use super::{
    commands::{
        app_state_command_for_serial, serial_command_event_and_responses, SchedulerOperation,
        SerialCommand,
    },
    io::{
        drain_app_state_apply_acks, drain_ui_cycle_step_acks, run_allocator_alloc_probe,
        run_sdwait_command, wait_app_state_apply_ack, wait_ui_cycle_step_ack,
        write_allocator_status_line, write_diag_status_line, write_sd_request_queued,
        write_state_status_line,
    },
    metrics,
    queue::{enqueue_app_event_with_retry, enqueue_sd_request_with_retry},
    task_state::SerialTaskState,
};
#[cfg(feature = "asset-upload-http")]
use netstack::config::{channels::NET_CONTROL_COMMANDS, NetControlCommand};

use crate::firmware::{
    config::{APP_STATE_APPLY_ACK_TIMEOUT_MS, UI_CYCLE_STEP_ACK_TIMEOUT_MS},
    observability,
    touch::debug_log::uart_write_all,
    types::{AppEvent, SdCommand, SdRequest, SerialWriter, UiCycleStepStatus, UiCycleTarget},
};

pub(super) fn queue_observation_fixture(
    command: crate::firmware::observation_fixture::ParsedCommand,
) {
    crate::firmware::observation_fixture::enqueue(command);
}

pub(super) async fn handle_serial_command(
    uart: &mut SerialWriter,
    state: &mut SerialTaskState,
    cmd: SerialCommand,
) {
    match cmd {
        SerialCommand::TimeReply {
            version,
            session,
            nonce,
            utc_epoch_seconds,
            offset_minutes,
        } => {
            let reply = wall_clock::message::SyncReply {
                version,
                session,
                nonce,
                utc_epoch_seconds,
                offset_minutes,
            };
            super::time_dispatch::run_time_reply_command(uart, state, reply).await;
        }
        SerialCommand::TimeSync => {
            super::time_dispatch::run_timesync_command(uart, state).await;
        }
        SerialCommand::TimeGet => {
            super::time_dispatch::run_timeget_command(uart, state).await;
        }
        SerialCommand::ObservationFixture(_) | SerialCommand::ObservationPanelCycle(_) => {
            unreachable!("observation fixture uses the allocation-free path");
        }
        SerialCommand::UiCycleStep { target } => {
            run_ui_cycle_step_command(uart, state, target).await;
        }
        #[cfg(feature = "ui-provider-fixture")]
        SerialCommand::UiProviderFixtureStep => {
            run_ui_provider_fixture_step_command(uart, state).await;
        }
        SerialCommand::FirmwareFactoryBoot => {
            unreachable!("firmware command reached ordinary boxed dispatcher");
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::BleProbeStart | SerialCommand::BleProbeStatus => {
            handle_local_command(uart, state, cmd).await;
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::BlePhase1sStart { .. }
        | SerialCommand::BlePhase1sStatus
        | SerialCommand::RadioHandoffAcquire { .. }
        | SerialCommand::RadioHandoffRelease { .. }
        | SerialCommand::RadioHandoffStatus => {
            unreachable!("low-overhead BLE lifecycle command reached boxed dispatcher");
        }
        SerialCommand::Metrics | SerialCommand::MetricsNet => {
            unreachable!("metrics command reached ordinary boxed dispatcher");
        }
        SerialCommand::Ping
        | SerialCommand::DisplayPause { .. }
        | SerialCommand::AcquisitionWindow
        | SerialCommand::TouchSchedReset
        | SerialCommand::Scheduler { .. }
        | SerialCommand::TelemetryStatus
        | SerialCommand::TelemetrySet { .. }
        | SerialCommand::AllocatorAllocProbe { .. }
        | SerialCommand::SdWait { .. }
        | SerialCommand::DiagGet
        | SerialCommand::StateGet => {
            handle_local_command(uart, state, cmd).await;
        }
        #[cfg(feature = "firmware-trace")]
        SerialCommand::Trace { .. } => handle_local_command(uart, state, cmd).await,
        SerialCommand::StackStatus
        | SerialCommand::ConsoleStats
        | SerialCommand::MountainStatus
        | SerialCommand::AllocatorStatus => {
            unreachable!("low-overhead memory command reached boxed dispatcher");
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::StateSet { .. }
        | SerialCommand::StateDiag { .. }
        | SerialCommand::NetStart
        | SerialCommand::NetStop => {
            unreachable!("low-overhead app-state command reached boxed dispatcher");
        }
        #[cfg(not(feature = "asset-upload-http"))]
        SerialCommand::StateSet { .. } | SerialCommand::StateDiag { .. } => {
            run_app_state_set_command(uart, state, cmd).await;
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetCfgSet { .. }
        | SerialCommand::NetCfgGet
        | SerialCommand::NetListenerSet { .. } => {
            handle_network_command(uart, cmd).await;
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetRecover => {
            unreachable!("network recovery uses the allocation-free path");
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetStatus => {
            unreachable!("low-overhead network status reached boxed dispatcher");
        }
        queued_command => dispatch_queued_command(uart, state, queued_command).await,
    }
}

#[cfg(feature = "ui-provider-fixture")]
async fn run_ui_provider_fixture_step_command(
    uart: &mut SerialWriter,
    state: &mut SerialTaskState,
) {
    drain_ui_cycle_step_acks();
    let request_id = state.next_state_request_id();
    let queued = enqueue_app_event_with_retry(AppEvent::UiProviderFixtureStep {
        ack_request_id: request_id,
    })
    .await;
    if !queued {
        let _ = uart_write_all(uart, b"UIFIXTURE BUSY reason=app_event_queue\r\n").await;
        return;
    }
    let Some(ack) = wait_ui_cycle_step_ack(request_id, UI_CYCLE_STEP_ACK_TIMEOUT_MS).await else {
        let _ = uart_write_all(uart, b"UIFIXTURE ERR reason=timeout ambiguous=true\r\n").await;
        return;
    };
    let response = match ack.status {
        UiCycleStepStatus::Applied => b"UIFIXTURE OK\r\n".as_slice(),
        UiCycleStepStatus::NotReady => b"UIFIXTURE ERR reason=not_ready\r\n".as_slice(),
        UiCycleStepStatus::Busy => b"UIFIXTURE BUSY reason=display_busy\r\n".as_slice(),
        UiCycleStepStatus::NavigationFault => {
            b"UIFIXTURE ERR reason=navigation_fault\r\n".as_slice()
        }
        UiCycleStepStatus::NoDirty => b"UIFIXTURE ERR reason=no_dirty\r\n".as_slice(),
        UiCycleStepStatus::RefreshFailed => b"UIFIXTURE ERR reason=refresh_failed\r\n".as_slice(),
    };
    let _ = uart_write_all(uart, response).await;
}

async fn run_ui_cycle_step_command(
    uart: &mut SerialWriter,
    state: &mut SerialTaskState,
    target: UiCycleTarget,
) {
    drain_ui_cycle_step_acks();
    let request_id = state.next_state_request_id();
    let queued = enqueue_app_event_with_retry(AppEvent::UiCycleStep {
        ack_request_id: request_id,
        target,
    })
    .await;
    if !queued {
        let _ = uart_write_all(uart, b"UISTEP BUSY reason=app_event_queue\r\n").await;
        return;
    }

    let Some(ack) = wait_ui_cycle_step_ack(request_id, UI_CYCLE_STEP_ACK_TIMEOUT_MS).await else {
        let _ = uart_write_all(uart, b"UISTEP ERR reason=timeout ambiguous=true\r\n").await;
        return;
    };
    let response = match ack.status {
        UiCycleStepStatus::Applied => b"UISTEP OK\r\n".as_slice(),
        UiCycleStepStatus::NotReady => b"UISTEP ERR reason=not_ready\r\n".as_slice(),
        UiCycleStepStatus::Busy => b"UISTEP BUSY reason=display_busy\r\n".as_slice(),
        UiCycleStepStatus::NavigationFault => b"UISTEP ERR reason=navigation_fault\r\n".as_slice(),
        UiCycleStepStatus::NoDirty => b"UISTEP ERR reason=no_dirty\r\n".as_slice(),
        UiCycleStepStatus::RefreshFailed => b"UISTEP ERR reason=refresh_failed\r\n".as_slice(),
    };
    let _ = uart_write_all(uart, response).await;
}

async fn handle_local_command(
    uart: &mut SerialWriter,
    state: &mut SerialTaskState,
    cmd: SerialCommand,
) {
    match cmd {
        SerialCommand::DisplayPause { paused } => {
            if let Some(paused) = paused {
                crate::firmware::display::set_timer_pause(paused);
            }
            let mut line = heapless::String::<96>::new();
            let _ = core::fmt::write(
                &mut line,
                format_args!(
                    "DISPLAYPAUSE paused={} uptime_ms={} lease_seconds=3600\r\n",
                    crate::firmware::display::timer_paused(),
                    embassy_time::Instant::now().as_millis(),
                ),
            );
            let _ = uart_write_all(uart, line.as_bytes()).await;
        }
        SerialCommand::Ping => {
            let _ = uart_write_all(uart, b"PONG\r\n").await;
        }
        SerialCommand::AcquisitionWindow => {
            crate::firmware::imu::request_qualification_window();
            let _ = uart_write_all(uart, b"ACQWINDOW OK active_ms=30000\r\n").await;
        }
        #[cfg(feature = "firmware-trace")]
        SerialCommand::Trace { .. } => unreachable!("trace command uses its isolated dispatcher"),
        SerialCommand::TouchSchedReset => {
            // Exclude the reset command's own UART response from the measurement window.
            // The serial task cannot process the next command until this branch returns.
            let _ = uart_write_all(uart, b"TOUCHSCHEDRESET OK\r\n").await;
            crate::firmware::touch::scheduling::reset();
            crate::firmware::imu::metrics::reset();
            crate::firmware::imu::timing::reset();
            crate::firmware::acquisition_metrics::reset(embassy_time::Instant::now().as_millis());
            crate::firmware::types::bus_owner::begin_runtime_timing();
            #[cfg(feature = "cpu-load")]
            cpu_load::reset_profile_timings();
        }
        SerialCommand::Scheduler { operation } => {
            match operation {
                SchedulerOperation::Status => {}
                SchedulerOperation::Automatic => {
                    crate::firmware::scheduling::set_override(None);
                }
                SchedulerOperation::Profile(profile) => {
                    crate::firmware::scheduling::set_override(Some(profile));
                }
            }
            let status = crate::firmware::scheduling::status();
            let mut line = heapless::String::<128>::new();
            let override_label = status
                .override_profile
                .map_or("auto", |profile| profile.label());
            let _ = write!(
                &mut line,
                "SCHEDPROFILE active={} automatic={} override={} runtime_ready={}\r\n",
                status.selected.label(),
                status.automatic.label(),
                override_label,
                if crate::firmware::scheduling::runtime_ready() {
                    "on"
                } else {
                    "off"
                },
            );
            let _ = uart_write_all(uart, line.as_bytes()).await;
        }
        SerialCommand::TelemetryStatus => {
            metrics::write_telemetry_status_line(uart).await;
        }
        SerialCommand::TelemetrySet { operation } => {
            metrics::run_telemetry_set_command(uart, operation).await;
        }
        SerialCommand::DiagGet => {
            write_diag_status_line(uart).await;
        }
        SerialCommand::StateGet => {
            write_state_status_line(uart).await;
        }
        SerialCommand::TimeReply { .. } | SerialCommand::TimeSync | SerialCommand::TimeGet => {
            unreachable!("clock commands use the top-level dispatcher");
        }
        SerialCommand::AllocatorAllocProbe { bytes } => {
            run_allocator_alloc_probe(uart, bytes as usize).await;
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::BleProbeStart => {
            let response = b"BLEPROBE ERR reason=phase1s_selected\r\n".as_slice();
            let _ = uart_write_all(uart, response).await;
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::BleProbeStatus => {
            let queues = esp_radio::queue_lifecycle_stats();
            let mut queue_line = heapless::String::<320>::new();
            let _ = write!(
                &mut queue_line,
                "BLE_QUEUE created={} active={} retired={} reclaimed={} in_flight={} active_payload={} retired_payload={} late_rejected={} unknown_rejected={} duplicate_retire={} epoch={} reclaim_fail={} slots={}/{}\r\n",
                queues.created,
                queues.active,
                queues.retired,
                queues.reclaimed,
                queues.in_flight,
                queues.active_payload_bytes,
                queues.retired_payload_bytes,
                queues.late_use_rejected,
                queues.unknown_use_rejected,
                queues.duplicate_retire,
                queues.active_reclaimable_epoch,
                queues.reclaim_failures,
                queues.slot_high_water,
                queues.slot_capacity,
            );
            let _ = uart_write_all(uart, queue_line.as_bytes()).await;
            queue_line.clear();
            let _ = write!(
                &mut queue_line,
                "BLE_QUEUE_OWNER created={} deleted={} active={} retired={} reclaimed={} corruption={} task_contention_rejected={} isr_contention_rejected={} nonblocking_context_redirected={} payload={}/{} slots={}/{}\r\n",
                queues.owner_created,
                queues.owner_deleted,
                queues.owner_active,
                queues.owner_retired,
                queues.owner_reclaimed,
                queues.owner_corruption,
                queues.owner_task_contention_rejected,
                queues.owner_isr_contention_rejected,
                queues.owner_nonblocking_context_redirected,
                queues.owner_payload_bytes,
                queues.owner_payload_capacity_bytes,
                queues.owner_slot_high_water,
                queues.owner_slot_capacity,
            );
            let _ = uart_write_all(uart, queue_line.as_bytes()).await;
            let mut line = heapless::String::<176>::new();
            let _ = write!(
                &mut line,
                "BLEPROBE state=failed cycle=0 failure=phase1s_selected build_id={} cycles=20 coex=true\r\n",
                crate::firmware::ble::phase1d_status().build_id,
            );
            let _ = uart_write_all(uart, line.as_bytes()).await;
        }
        SerialCommand::SdWait { target, timeout_ms } => {
            let last_sd_request_id = state.last_sd_request_id();
            run_sdwait_command(
                uart,
                state.sd_result_cache_mut(),
                last_sd_request_id,
                target,
                timeout_ms,
            )
            .await;
        }
        _ => unreachable!("local serial command must map to local dispatch"),
    }
}

#[cfg(feature = "firmware-trace")]
pub(super) async fn handle_trace_command(uart: &mut SerialWriter, operation: u8) {
    crate::firmware::trace::command(uart, operation).await;
}

#[cfg(feature = "ble-foundation")]
async fn run_radio_handoff_command(
    uart: &mut SerialWriter,
    command: arbitration::handoff::NetworkOwnerCommand,
) {
    let timeout = if matches!(command, arbitration::handoff::NetworkOwnerCommand::Status) {
        embassy_time::Duration::from_secs(3)
    } else {
        embassy_time::Duration::from_secs(200)
    };
    let Ok(ticket) = netstack::request_handoff(command) else {
        let _ = uart_write_all(
            uart,
            b"RADIO_HANDOFF_ACK kind=rejected reason=busy state=unknown\r\n",
        )
        .await;
        return;
    };
    let Ok(ack) = embassy_time::with_timeout(timeout, netstack::receive_handoff_ack(ticket)).await
    else {
        netstack::cancel_handoff_request(ticket);
        let _ = uart_write_all(
            uart,
            b"RADIO_HANDOFF_ACK kind=faulted reason=timeout ambiguous=true\r\n",
        )
        .await;
        return;
    };
    let (kind, reason) = match ack.kind {
        arbitration::handoff::NetworkOwnerAckKind::Status => ("status", "none"),
        arbitration::handoff::NetworkOwnerAckKind::Quiesced => ("quiesced", "none"),
        arbitration::handoff::NetworkOwnerAckKind::Restored => ("restored", "none"),
        arbitration::handoff::NetworkOwnerAckKind::Rejected(reason) => ("rejected", reason.label()),
        arbitration::handoff::NetworkOwnerAckKind::Faulted(reason) => ("faulted", reason.label()),
    };
    let mut line = heapless::String::<640>::new();
    let _ = write!(
        &mut line,
        "RADIO_HANDOFF_ACK kind={} state={} reason={} boot={} epoch={} internal_free={} block_above_reserve={} probe_before={} probe_after={} probe_reserve={} http={} sd_roundtrip={} sd_session={} callbacks={} queues={} source_active={} callback_admission={} late_callbacks={} queue_late={} queue_unknown={} queue_reclaim_fail={} queue_corruption={} queue_contention={} stable={}\r\n",
        kind,
        ack.state.label(),
        reason,
        ack.boot_generation,
        ack.epoch,
        ack.resources.internal_free_bytes,
        ack.resources.largest_block_above_reserve_bytes,
        ack.resources.probe_free_before_bytes,
        ack.resources.probe_free_after_bytes,
        ack.resources.probe_reserve_bytes,
        ack.resources.service_connections,
        ack.resources.storage_roundtrips,
        ack.resources.storage_sessions,
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
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(super) async fn metrics_command(uart: &mut SerialWriter) {
    metrics::write_metrics_lines(uart).await;
}

pub(super) async fn metrics_net_command(uart: &mut SerialWriter) {
    metrics::write_metrics_net_lines(uart).await;
}

pub(super) async fn handle_firmware_command(
    uart: &mut SerialWriter,
    state: &mut SerialTaskState,
    cmd: SerialCommand,
) {
    let _ = state;
    match cmd {
        SerialCommand::FirmwareFactoryBoot => {
            match crate::firmware::update::request_factory_boot() {
                Ok(()) => {
                    let _ = uart_write_all(uart, b"FWFACTORYBOOT OK rebooting=yes\r\n").await;
                    esp_hal::system::software_reset();
                }
                Err(error) => write_firmware_error(uart, "FWFACTORYBOOT", error).await,
            }
        }
        _ => unreachable!("firmware command must map to firmware dispatch"),
    }
}

pub(super) async fn release_firmware_update_hardware(state: &mut SerialTaskState) {
    if state.end_firmware_update_hardware_lease() {
        let _ = crate::firmware::panel_bus::try_request_clients_resume(true);
    }
}

async fn write_firmware_error(
    uart: &mut SerialWriter,
    command: &str,
    error: crate::firmware::update::UpdateError,
) {
    let mut line = heapless::String::<96>::new();
    let _ = write!(&mut line, "{} ERR reason={}\r\n", command, error.label());
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

#[cfg(feature = "asset-upload-http")]
async fn handle_network_command(uart: &mut SerialWriter, cmd: SerialCommand) {
    match cmd {
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetCfgSet { config } => {
            run_netcfg_set_command(uart, config).await;
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetCfgGet => {
            run_netcfg_get_command(uart).await;
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetStatus => {
            unreachable!("network status is handled by the low-overhead dispatcher");
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetListenerSet { enabled } => {
            run_net_listener_set_command(uart, enabled).await;
        }
        SerialCommand::NetRecover => unreachable!("network recovery is allocation-free"),
        _ => unreachable!("network serial command must map to network dispatch"),
    }
}

// `state` is only read by the asset-upload-http-gated app-state arms below.
#[cfg_attr(not(feature = "asset-upload-http"), allow(unused_variables))]
pub(super) async fn run_low_overhead_diagnostic_command(
    uart: &mut SerialWriter,
    state: &mut SerialTaskState,
    command: SerialCommand,
) {
    match command {
        SerialCommand::StackStatus => write_stack_status_line(uart).await,
        SerialCommand::ConsoleStats => write_console_drop_status_line(uart).await,
        SerialCommand::MountainStatus => write_mountain_status_line(uart).await,
        SerialCommand::AllocatorStatus => write_allocator_status_line(uart).await,
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetStatus => write_net_status_line(uart).await,
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetRecover => recover_network(uart).await,
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::StateSet { .. }
        | SerialCommand::StateDiag { .. }
        | SerialCommand::NetStart
        | SerialCommand::NetStop => run_app_state_set_command(uart, state, command).await,
        #[cfg(feature = "ble-foundation")]
        SerialCommand::BlePhase1sStart {
            boot_generation,
            epoch,
        } => {
            let mut line = heapless::String::<112>::new();
            let result = crate::firmware::ble::request_phase1s_probe(boot_generation, epoch);
            match result {
                Ok(()) => {
                    let _ = write!(
                        &mut line,
                        "BLE_P1S_ACK kind=queued reason=none boot={} epoch={}\r\n",
                        boot_generation, epoch
                    );
                }
                Err(error) => {
                    let reason = match error {
                        crate::firmware::ble::ProbeRequestError::Busy => "busy",
                        crate::firmware::ble::ProbeRequestError::Consumed => "consumed",
                        crate::firmware::ble::ProbeRequestError::OwnershipUnknown => {
                            "ownership_unknown"
                        }
                        crate::firmware::ble::ProbeRequestError::ExclusiveLease => {
                            "exclusive_lease"
                        }
                        crate::firmware::ble::ProbeRequestError::UpdateReserved => {
                            "update_reserved"
                        }
                    };
                    let _ = write!(
                        &mut line,
                        "BLE_P1S_ACK kind=rejected reason={} boot={} epoch={}\r\n",
                        reason, boot_generation, epoch
                    );
                }
            }
            let _ = uart_write_all(uart, line.as_bytes()).await;
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::BlePhase1sStatus => write_ble_phase1s_status(uart).await,
        #[cfg(feature = "ble-foundation")]
        SerialCommand::RadioHandoffAcquire {
            boot_generation,
            epoch,
        } => {
            run_radio_handoff_command(
                uart,
                arbitration::handoff::NetworkOwnerCommand::AcquireExclusive {
                    boot_generation,
                    epoch,
                },
            )
            .await;
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::RadioHandoffRelease {
            boot_generation,
            epoch,
        } => {
            run_radio_handoff_command(
                uart,
                arbitration::handoff::NetworkOwnerCommand::ReleaseExclusive {
                    boot_generation,
                    epoch,
                },
            )
            .await;
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::RadioHandoffStatus => {
            run_radio_handoff_command(uart, arbitration::handoff::NetworkOwnerCommand::Status)
                .await;
        }
        _ => unreachable!("only low-overhead diagnostics bypass the boxed dispatcher"),
    }
}

#[cfg(feature = "asset-upload-http")]
async fn recover_network(uart: &mut SerialWriter) {
    while NET_CONTROL_COMMANDS.try_receive().is_ok() {}
    if NET_CONTROL_COMMANDS
        .try_send(NetControlCommand::Recover)
        .is_ok()
    {
        let _ = uart_write_all(uart, b"NET OK op=recover\r\n").await;
    } else {
        let _ = uart_write_all(uart, b"NET ERR reason=busy\r\n").await;
    }
}

#[cfg(feature = "asset-upload-http")]
async fn write_net_status_line(uart: &mut SerialWriter) {
    let line = netstack::config::format_net_status_line();
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

#[cfg(feature = "ble-foundation")]
async fn write_ble_phase1s_status(uart: &mut SerialWriter) {
    let status = crate::firmware::ble::phase1d_status();
    let mut line = heapless::String::<768>::new();
    let formatted = write!(
        &mut line,
        "BLE_P1S_STATUS state={} failure={} build_id={} boot={} epoch={} before={} controller={} active={} after={} callbacks={} admission={} rejected={} rx_overflow={} rx_oversize={} tx_rejected={} tx_timeout={} queues={} queue_late={} queue_unknown={} queue_reclaim={} queue_corruption={} queue_contention={} queue_task_cancelled={} queue_balance={} queue_task_live={} queue_task_faults={} queue_op_full={} transport_faulted={} packets_free={} pool_exhausted={} coex=false\r\n",
        status.state_label(),
        status.failure_label(),
        status.build_id,
        status.boot_generation,
        status.epoch,
        status.before_free,
        status.controller_free,
        status.active_free,
        status.after_free,
        status.callbacks_in_flight,
        status.callback_admission,
        status.callbacks_rejected,
        status.rx_queue_overflow,
        status.rx_oversize,
        status.tx_rejected,
        status.tx_timeout,
        status.queues_active,
        status.queue_late_use,
        status.queue_unknown_use,
        status.queue_reclaim_failures,
        status.queue_corruption,
        status.queue_contention,
        status.queue_task_cancelled,
        status.queue_operation_balance_error,
        status.queue_task_live,
        status.queue_task_faults,
        status.queue_operation_registry_full,
        status.transport_faulted,
        status.packets_free,
        status.pool_exhausted,
    );
    if formatted.is_err() {
        let _ = uart_write_all(uart, b"BLE_P1S_STATUS ERR reason=response_overflow\r\n").await;
        return;
    }
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

async fn write_stack_status_line(uart: &mut SerialWriter) {
    let mut line = heapless::String::<64>::new();
    let _ = write!(
        &mut line,
        "STACK_STATUS cpu0={} touch={} tx_drop={}\r\n",
        observability::minimum_stack_headroom_bytes(),
        observability::minimum_touch_core_stack_headroom_bytes(),
        crate::dropped_write_count(),
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

async fn write_console_drop_status_line(uart: &mut SerialWriter) {
    let counts = console::drop_counts();
    let mut line = heapless::String::<160>::new();
    if write!(
        &mut line,
        "CONSOLE_DROPS total={} contention={} deferred_overflow={} deferred_oversize={} stable={}\r\n",
        counts.total,
        counts.contention,
        counts.deferred_overflow,
        counts.deferred_oversize,
        counts.stable,
    )
    .is_err()
    {
        let _ = uart_write_all(uart, b"CONSOLE_DROPS ERR reason=response_overflow\r\n").await;
        return;
    }
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

async fn write_mountain_status_line(uart: &mut SerialWriter) {
    let (adopted, composed) = crate::firmware::ui::ambient_mountain_status();
    let mut line = heapless::String::<80>::new();
    let _ = write!(
        &mut line,
        "MOUNTAIN_STATUS adopted={} composed={} tx_drop={}\r\n",
        u8::from(adopted),
        u8::from(composed),
        crate::dropped_write_count(),
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

#[cfg(feature = "asset-upload-http")]
async fn run_net_listener_set_command(uart: &mut SerialWriter, enabled: bool) {
    let previous_enabled = netstack::host::listener_enabled();
    let seq_before = netstack::host::listener_set_seq();
    netstack::host::set_listener_enabled(enabled);
    let seq_after = netstack::host::listener_set_seq();
    if crate::firmware::observability::log_filter_enabled(
        crate::firmware::observability::LOG_DOMAIN_NET,
    ) {
        console::println!(
            "upload_http: listener_control cmd={} prev_enabled={} next_enabled={} seq_before={} seq_after={}",
            if enabled { "on" } else { "off" },
            previous_enabled,
            enabled,
            seq_before,
            seq_after,
        );
    }
    let response = if enabled {
        b"NET OK op=listener_on\r\n".as_slice()
    } else {
        b"NET OK op=listener_off\r\n".as_slice()
    };
    let _ = uart_write_all(uart, response).await;
}

async fn dispatch_queued_command(
    uart: &mut SerialWriter,
    state: &mut SerialTaskState,
    cmd: SerialCommand,
) {
    let (app_event, sd_command, ok_response, busy_response) =
        serial_command_event_and_responses(cmd);
    let mut sd_request_meta: Option<(u32, SdCommand)> = None;
    let queued = if let Some(event) = app_event {
        enqueue_app_event_with_retry(event).await
    } else if let Some(command) = sd_command {
        let request_id = state.next_sd_request_id();
        let request = SdRequest {
            id: request_id,
            command,
        };
        sd_request_meta = Some((request_id, command));
        enqueue_sd_request_with_retry(request).await
    } else {
        unreachable!("serial command must map to app or sd dispatch");
    };

    if queued {
        let _ = uart_write_all(uart, ok_response).await;
        if let Some((request_id, command)) = sd_request_meta {
            state.set_last_sd_request_id(request_id);
            write_sd_request_queued(uart, request_id, command).await;
        }
    } else {
        let _ = uart_write_all(uart, busy_response).await;
    }
}

async fn run_app_state_set_command(
    uart: &mut SerialWriter,
    state: &mut SerialTaskState,
    cmd: SerialCommand,
) {
    // Receipt trace. Uses a non-STATE prefix on purpose: the host harness
    // accepts any STATE-prefixed line as the command ack.
    let _ = uart_write_all(uart, b"SERCMD rx=state_set\r\n").await;
    let command =
        app_state_command_for_serial(cmd).expect("state commands must map to app-state updates");
    let responses = app_state_set_responses(cmd);

    drain_app_state_apply_acks();
    let request_id = state.next_state_request_id();
    let queued = enqueue_app_event_with_retry(AppEvent::ApplyAppStateCommand {
        command,
        ack_request_id: Some(request_id),
    })
    .await;
    if !queued {
        let _ = uart_write_all(uart, responses.busy).await;
        return;
    }

    if let Some(ack) = wait_app_state_apply_ack(request_id, APP_STATE_APPLY_ACK_TIMEOUT_MS).await {
        if ack.status == 2 {
            let _ = uart_write_all(uart, responses.invalid_transition).await;
        } else {
            let _ = uart_write_all(uart, responses.ok).await;
        }
    } else {
        let _ = uart_write_all(uart, responses.timeout).await;
    }
}

struct AppStateSetResponses {
    ok: &'static [u8],
    busy: &'static [u8],
    timeout: &'static [u8],
    invalid_transition: &'static [u8],
}

fn app_state_set_responses(_cmd: SerialCommand) -> AppStateSetResponses {
    #[cfg(feature = "asset-upload-http")]
    if matches!(_cmd, SerialCommand::NetStart) {
        return AppStateSetResponses {
            ok: b"NET OK op=start\r\n",
            busy: b"NET ERR reason=busy\r\n",
            timeout: b"NET ERR reason=timeout\r\n",
            invalid_transition: b"NET ERR reason=invalid_transition\r\n",
        };
    }

    #[cfg(feature = "asset-upload-http")]
    if matches!(_cmd, SerialCommand::NetStop) {
        return AppStateSetResponses {
            ok: b"NET OK op=stop\r\n",
            busy: b"NET ERR reason=busy\r\n",
            timeout: b"NET ERR reason=timeout\r\n",
            invalid_transition: b"NET ERR reason=invalid_transition\r\n",
        };
    }

    AppStateSetResponses {
        ok: b"STATE OK\r\n",
        busy: b"STATE BUSY\r\n",
        timeout: b"STATE ERR reason=timeout\r\n",
        invalid_transition: b"STATE ERR reason=invalid_transition\r\n",
    }
}
