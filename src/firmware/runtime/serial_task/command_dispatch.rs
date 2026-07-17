use core::fmt::Write;

use super::{
    commands::{
        app_state_command_for_serial, serial_command_event_and_responses, SchedulerOperation,
        SerialCommand,
    },
    io::{
        drain_app_state_apply_acks, run_allocator_alloc_probe, run_netcfg_get_command,
        run_netcfg_set_command, run_sdwait_command, wait_app_state_apply_ack,
        write_allocator_status_line, write_diag_status_line, write_sd_request_queued,
        write_state_status_line,
    },
    metrics,
    queue::{enqueue_app_event_with_retry, enqueue_sd_request_with_retry},
    task_state::SerialTaskState,
};
use crate::firmware::{
    config::{APP_STATE_APPLY_ACK_TIMEOUT_MS, NET_CONTROL_COMMANDS},
    runtime::service_mode,
    storage::upload::wifi,
    touch::debug_log::uart_write_all,
    types::{AppEvent, NetControlCommand, SdCommand, SdRequest, SerialUart},
};

pub(super) async fn handle_serial_command(
    uart: &mut SerialUart,
    state: &mut SerialTaskState,
    cmd: SerialCommand,
) {
    match cmd {
        #[cfg(not(feature = "wifi-debug-slim-app"))]
        SerialCommand::TouchWizardDump => {
            state.write_touch_wizard_dump(uart).await;
        }
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
                    crate::firmware::runtime::scheduling::set_override(None);
                }
                SchedulerOperation::Profile(profile) => {
                    crate::firmware::runtime::scheduling::set_override(Some(profile));
                }
            }
            let status = crate::firmware::runtime::scheduling::status();
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
                if crate::firmware::runtime::scheduling::runtime_ready() {
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
        SerialCommand::AllocatorAllocProbe { bytes } => {
            run_allocator_alloc_probe(uart, bytes as usize).await;
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
            let previous_enabled = service_mode::upload_http_listener_enabled();
            let seq_before = service_mode::upload_http_listener_set_seq();
            service_mode::set_upload_http_listener_enabled(enabled);
            let seq_after = service_mode::upload_http_listener_set_seq();
            if crate::firmware::telemetry::diag_enabled(crate::firmware::telemetry::DIAG_DOMAIN_NET)
            {
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
        _ => {
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

fn app_state_set_responses(cmd: SerialCommand) -> AppStateSetResponses {
    #[cfg(feature = "asset-upload-http")]
    if matches!(cmd, SerialCommand::NetStart) {
        return AppStateSetResponses {
            ok: b"NET OK op=start\r\n",
            busy: b"NET ERR reason=busy\r\n",
            timeout: b"NET ERR reason=timeout\r\n",
            invalid_transition: b"NET ERR reason=invalid_transition\r\n",
        };
    }

    #[cfg(feature = "asset-upload-http")]
    if matches!(cmd, SerialCommand::NetStop) {
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
