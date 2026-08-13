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
use crate::firmware::{
    config::NET_CONTROL_COMMANDS, net::wifi, service_mode, types::NetControlCommand,
};
use crate::firmware::{
    config::{APP_STATE_APPLY_ACK_TIMEOUT_MS, UI_CYCLE_STEP_ACK_TIMEOUT_MS},
    touch::debug_log::uart_write_all,
    types::{AppEvent, SdCommand, SdRequest, SerialUart, UiCycleStepStatus},
};

pub(super) async fn handle_serial_command(
    uart: &mut SerialUart,
    state: &mut SerialTaskState,
    cmd: SerialCommand,
) {
    match cmd {
        SerialCommand::UiCycleStep => {
            run_ui_cycle_step_command(uart, state).await;
        }
        #[cfg(feature = "ui-provider-fixture")]
        SerialCommand::UiProviderFixtureStep => {
            run_ui_provider_fixture_step_command(uart, state).await;
        }
        SerialCommand::FirmwareStatus
        | SerialCommand::FirmwarePrepare
        | SerialCommand::FirmwareBegin { .. }
        | SerialCommand::FirmwareChunk { .. }
        | SerialCommand::FirmwareStream { .. }
        | SerialCommand::FirmwareFinish
        | SerialCommand::FirmwareActivate
        | SerialCommand::FirmwareAbort => {
            handle_firmware_command(uart, state, cmd).await;
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::BleProbeStart | SerialCommand::BleProbeStatus => {
            handle_local_command(uart, state, cmd).await;
        }
        SerialCommand::Ping
        | SerialCommand::Metrics
        | SerialCommand::TouchSchedReset
        | SerialCommand::Scheduler { .. }
        | SerialCommand::MetricsNet
        | SerialCommand::TelemetryStatus
        | SerialCommand::TelemetrySet { .. }
        | SerialCommand::AllocatorStatus
        | SerialCommand::AllocatorAllocProbe { .. }
        | SerialCommand::SdWait { .. }
        | SerialCommand::DiagGet
        | SerialCommand::StateGet => {
            handle_local_command(uart, state, cmd).await;
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::StateSet { .. }
        | SerialCommand::StateDiag { .. }
        | SerialCommand::NetStart
        | SerialCommand::NetStop => {
            run_app_state_set_command(uart, state, cmd).await;
        }
        #[cfg(not(feature = "asset-upload-http"))]
        SerialCommand::StateSet { .. } | SerialCommand::StateDiag { .. } => {
            run_app_state_set_command(uart, state, cmd).await;
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetCfgSet { .. }
        | SerialCommand::NetCfgGet
        | SerialCommand::NetStatus
        | SerialCommand::NetListenerSet { .. }
        | SerialCommand::NetRecover => {
            handle_network_command(uart, cmd).await;
        }
        queued_command => dispatch_queued_command(uart, state, queued_command).await,
    }
}

#[cfg(feature = "ui-provider-fixture")]
async fn run_ui_provider_fixture_step_command(uart: &mut SerialUart, state: &mut SerialTaskState) {
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

async fn run_ui_cycle_step_command(uart: &mut SerialUart, state: &mut SerialTaskState) {
    drain_ui_cycle_step_acks();
    let request_id = state.next_state_request_id();
    let queued = enqueue_app_event_with_retry(AppEvent::UiCycleStep {
        ack_request_id: request_id,
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
    uart: &mut SerialUart,
    state: &mut SerialTaskState,
    cmd: SerialCommand,
) {
    match cmd {
        SerialCommand::Ping => {
            let _ = uart_write_all(uart, b"PONG\r\n").await;
        }
        SerialCommand::Metrics => {
            metrics::write_metrics_lines(uart).await;
        }
        SerialCommand::TouchSchedReset => {
            // Exclude the reset command's own UART response from the measurement window.
            // The serial task cannot process the next command until this branch returns.
            let _ = uart_write_all(uart, b"TOUCHSCHEDRESET OK\r\n").await;
            crate::firmware::touch::scheduling::reset();
            crate::firmware::imu::metrics::reset();
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
        SerialCommand::MetricsNet => {
            metrics::write_metrics_net_lines(uart).await;
        }
        SerialCommand::TelemetryStatus => {
            metrics::write_telemetry_status_line(uart).await;
        }
        SerialCommand::TelemetrySet { operation } => {
            metrics::run_telemetry_set_command(uart, operation).await;
        }
        SerialCommand::AllocatorStatus => {
            write_allocator_status_line(uart).await;
        }
        SerialCommand::DiagGet => {
            write_diag_status_line(uart).await;
        }
        SerialCommand::StateGet => {
            write_state_status_line(uart).await;
        }
        SerialCommand::AllocatorAllocProbe { bytes } => {
            run_allocator_alloc_probe(uart, bytes as usize).await;
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::BleProbeStart => {
            let response = match crate::firmware::ble::request_phase1d_probe() {
                Ok(()) => b"BLEPROBE QUEUED\r\n".as_slice(),
                Err(crate::firmware::ble::ProbeRequestError::Busy) => {
                    b"BLEPROBE BUSY\r\n".as_slice()
                }
                Err(crate::firmware::ble::ProbeRequestError::OwnershipUnknown) => {
                    b"BLEPROBE ERR reason=ownership_unknown reboot_required=true\r\n".as_slice()
                }
            };
            let _ = uart_write_all(uart, response).await;
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::BleProbeStatus => {
            let status = crate::firmware::ble::phase1d_status();
            let mut line = heapless::String::<176>::new();
            let _ = write!(
                &mut line,
                "BLEPROBE state={} cycle={} failure={} build_id={} cycles={} coex=true\r\n",
                status.state_label(),
                status.cycle,
                status.failure_label(),
                status.build_id,
                status.cycles,
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

async fn handle_firmware_command(
    uart: &mut SerialUart,
    state: &mut SerialTaskState,
    cmd: SerialCommand,
) {
    match cmd {
        SerialCommand::FirmwareStatus => write_firmware_status(uart).await,
        SerialCommand::FirmwarePrepare => prepare_firmware_update(uart, state).await,
        SerialCommand::FirmwareBegin {
            image_len,
            digest,
            signature,
        } => match crate::firmware::update::begin(image_len, digest, signature) {
            Ok(target) => {
                let mut line = heapless::String::<96>::new();
                let _ = write!(
                    &mut line,
                    "FWBEGIN OK target={} total={}\r\n",
                    target.label(),
                    image_len,
                );
                let _ = uart_write_all(uart, line.as_bytes()).await;
            }
            Err(error) => write_firmware_error(uart, "FWBEGIN", error).await,
        },
        SerialCommand::FirmwareChunk { offset, bytes, len } => {
            match crate::firmware::update::write_chunk(offset, &bytes[..len as usize]) {
                Ok(written) => {
                    let mut line = heapless::String::<64>::new();
                    let _ = write!(&mut line, "FWCHUNK OK written={}\r\n", written);
                    let _ = uart_write_all(uart, line.as_bytes()).await;
                }
                Err(error) => write_firmware_error(uart, "FWCHUNK", error).await,
            }
        }
        SerialCommand::FirmwareStream { baud } => {
            super::firmware_stream::begin_stream(uart, state, baud).await;
        }
        SerialCommand::FirmwareFinish => finish_firmware_update(uart, state).await,
        SerialCommand::FirmwareActivate => match crate::firmware::update::activate() {
            Ok(target) => {
                let mut line = heapless::String::<80>::new();
                let _ = write!(
                    &mut line,
                    "FWACTIVATE OK target={} rebooting=yes\r\n",
                    target.label(),
                );
                let _ = uart_write_all(uart, line.as_bytes()).await;
                esp_hal::system::software_reset();
            }
            Err(error) => write_firmware_error(uart, "FWACTIVATE", error).await,
        },
        SerialCommand::FirmwareAbort => {
            crate::firmware::update::abort();
            let _ = uart_write_all(uart, b"FWABORT OK\r\n").await;
            release_firmware_update_hardware(state).await;
            crate::firmware::update::end_transport();
        }
        _ => unreachable!("firmware command must map to firmware dispatch"),
    }
}

async fn prepare_firmware_update(uart: &mut SerialUart, state: &mut SerialTaskState) {
    crate::firmware::update::prepare_transport();
    if state.begin_firmware_update_hardware_lease() {
        crate::firmware::panel_bus::suspend_clients().await;
        crate::firmware::flash::park_other_core_for_update();
    }
    let _ = uart_write_all(
        uart,
        b"FWPREPARE OK quiet=yes panel_clients=suspended other_core=parked\r\n",
    )
    .await;
}

async fn finish_firmware_update(uart: &mut SerialUart, state: &mut SerialTaskState) {
    match crate::firmware::update::finish() {
        Ok(digest) => {
            let mut line = heapless::String::<96>::new();
            let _ = line.push_str("FWFINISH OK sha256=");
            for byte in digest {
                let _ = write!(&mut line, "{byte:02x}");
            }
            let _ = line.push_str("\r\n");
            let _ = uart_write_all(uart, line.as_bytes()).await;
            release_firmware_update_hardware(state).await;
        }
        Err(error) => {
            write_firmware_error(uart, "FWFINISH", error).await;
            release_firmware_update_hardware(state).await;
            crate::firmware::update::end_transport();
        }
    }
}

async fn release_firmware_update_hardware(state: &mut SerialTaskState) {
    let _ = crate::firmware::flash::unpark_other_core_after_update();
    if state.end_firmware_update_hardware_lease() {
        let _ = crate::firmware::panel_bus::try_request_clients_resume(true);
    }
}

async fn write_firmware_status(uart: &mut SerialUart) {
    match crate::firmware::update::status() {
        Ok(status) => {
            let end_transport = status.phase == crate::firmware::update::SessionPhase::Verified;
            let mut line = heapless::String::<320>::new();
            let _ = write!(
                &mut line,
                "FWSTATUS build_id={} booted={} selected={} state={} phase={} target={} written={} total={} key={} key_id={:02x}{:02x}{:02x}{:02x} erase_max_us={} write_max_us={} verify_read_us={} multicore=transaction_park stream=bin1@{}\r\n",
                status.build_id,
                status.booted.label(),
                status.selected.map_or("none", |slot| slot.label()),
                status.image_state.map_or("none", crate::firmware::update::image_state_label),
                status.phase.label(),
                status.target.map_or("none", |slot| slot.label()),
                status.written,
                status.total,
                if status.public_key_configured { "configured" } else { "missing" },
                status.public_key_id[0],
                status.public_key_id[1],
                status.public_key_id[2],
                status.public_key_id[3],
                status.max_erase_us,
                status.max_write_us,
                status.verify_read_us,
                super::firmware_stream::STREAM_BAUD,
            );
            let _ = uart_write_all(uart, line.as_bytes()).await;
            if end_transport {
                crate::firmware::update::end_transport();
            }
        }
        Err(error) => write_firmware_error(uart, "FWSTATUS", error).await,
    }
}

async fn write_firmware_error(
    uart: &mut SerialUart,
    command: &str,
    error: crate::firmware::update::UpdateError,
) {
    let mut line = heapless::String::<96>::new();
    let _ = write!(&mut line, "{} ERR reason={}\r\n", command, error.label());
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

#[cfg(feature = "asset-upload-http")]
async fn handle_network_command(uart: &mut SerialUart, cmd: SerialCommand) {
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
            let status = wifi::net_status_snapshot();
            let mut line = heapless::String::<320>::new();
            let _ = write!(
                &mut line,
                "NET_STATUS {{\"state\":\"{}\",\"link\":{},\"radio_quiesced\":{},\"ipv4\":\"{}.{}.{}.{}\",\"listener\":{},\"listener_enabled\":{},\"failure_class\":\"{}\",\"failure_code\":{},\"ladder_step\":\"{}\",\"attempt\":{},\"uptime_ms\":{}}}\r\n",
                status.state,
                if status.link { "true" } else { "false" },
                if status.radio_quiesced {
                    "true"
                } else {
                    "false"
                },
                status.ipv4[0],
                status.ipv4[1],
                status.ipv4[2],
                status.ipv4[3],
                if status.listener { "true" } else { "false" },
                if status.listener_enabled {
                    "true"
                } else {
                    "false"
                },
                status.failure_class,
                status.failure_code,
                status.ladder_step,
                status.attempt,
                status.uptime_ms,
            );
            let _ = uart_write_all(uart, line.as_bytes()).await;
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetListenerSet { enabled } => {
            run_net_listener_set_command(uart, enabled).await;
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetRecover => {
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
        _ => unreachable!("network serial command must map to network dispatch"),
    }
}

#[cfg(feature = "asset-upload-http")]
async fn run_net_listener_set_command(uart: &mut SerialUart, enabled: bool) {
    let previous_enabled = service_mode::upload_http_listener_enabled();
    let seq_before = service_mode::upload_http_listener_set_seq();
    service_mode::set_upload_http_listener_enabled(enabled);
    let seq_after = service_mode::upload_http_listener_set_seq();
    if crate::firmware::observability::log_filter_enabled(
        crate::firmware::observability::LOG_DOMAIN_NET,
    ) {
        esp_println::println!(
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
    let _ = uart.write_async(response).await;
}

async fn dispatch_queued_command(
    uart: &mut SerialUart,
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
        let _ = uart.write_async(ok_response).await;
        if let Some((request_id, command)) = sd_request_meta {
            state.set_last_sd_request_id(request_id);
            write_sd_request_queued(uart, request_id, command).await;
        }
    } else {
        let _ = uart.write_async(busy_response).await;
    }
}

async fn run_app_state_set_command(
    uart: &mut SerialUart,
    state: &mut SerialTaskState,
    cmd: SerialCommand,
) {
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
        let _ = uart.write_async(responses.busy).await;
        return;
    }

    if let Some(ack) = wait_app_state_apply_ack(request_id, APP_STATE_APPLY_ACK_TIMEOUT_MS).await {
        if ack.status == 2 {
            let _ = uart.write_async(responses.invalid_transition).await;
        } else {
            let _ = uart.write_async(responses.ok).await;
        }
    } else {
        let _ = uart.write_async(responses.timeout).await;
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
