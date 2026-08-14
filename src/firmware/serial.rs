extern crate alloc;

mod command_dispatch;
mod command_family;
mod commands;
mod firmware_stream;
mod io;
mod labels;
mod line_reader;
mod metrics;
mod parser;
mod queue;
mod status;
mod task_state;

use core::pin::Pin;

use embassy_futures::yield_now;
use embassy_time::{with_timeout, Duration};

use commands::SerialCommand;
use line_reader::{LineReadEvent, SerialLineReader};
use parser::parse_serial_command;
use task_state::SerialTaskState;

use super::{touch::debug_log::uart_write_all, types::SerialUart};

#[embassy_executor::task]
pub(crate) async fn serial_task(mut uart: SerialUart) {
    let mut line_reader = SerialLineReader::new();
    let mut frame_reader = firmware_stream::FirmwareFrameReader::new();
    let mut rx = [0u8; 128];
    let mut state = SerialTaskState::new();

    state.write_trace_headers(&mut uart).await;

    loop {
        state.drain_runtime_samples(&mut uart).await;

        match with_timeout(Duration::from_millis(10), uart.read_async(&mut rx)).await {
            Ok(Ok(read)) => {
                for byte in rx[..read].iter().copied() {
                    if state.firmware_stream_active() {
                        let event = frame_reader.push_byte(byte);
                        if !matches!(&event, firmware_stream::FrameReadEvent::None) {
                            firmware_stream::handle_frame(&mut uart, &mut state, event).await;
                        }
                        if !state.firmware_stream_active() {
                            frame_reader.reset();
                        }
                    } else {
                        handle_uart_byte(&mut uart, &mut line_reader, &mut state, byte).await;
                    }
                }
            }
            Ok(Err(error)) => {
                if state.firmware_stream_active() {
                    frame_reader.reset();
                } else {
                    line_reader.discard_until_line_end();
                }
                esp_println::println!("SERIAL_RX status=error reason={error:?}");
            }
            Err(_) => {}
        }
        yield_now().await;
    }
}

async fn handle_uart_byte(
    uart: &mut SerialUart,
    line_reader: &mut SerialLineReader,
    state: &mut SerialTaskState,
    byte: u8,
) {
    match line_reader.push_byte(byte) {
        LineReadEvent::None => {}
        LineReadEvent::Overflow => {
            let _ = uart_write_all(uart, b"CMD ERR reason=overflow\r\n").await;
        }
        LineReadEvent::Complete(line) => {
            if let Some(cmd) = parse_serial_command(line) {
                if command_family::pre_box_command_class(&cmd)
                    == command_family::PreBoxCommandClass::Firmware
                {
                    let command_tag = command_family::firmware_command_tag(&cmd);
                    // Firmware update commands retain large stream/verification state and may
                    // execute while the flash cache is disabled. Keep that family in its own
                    // internal allocation so its maximum future size does not inflate every
                    // ordinary serial command, and never place it in external PSRAM.
                    let allocation =
                        crate::firmware::psram::InternalValue::try_new_bounded::<900, _>(|| {
                            command_dispatch::handle_firmware_command(uart, state, cmd)
                        });
                    if allocation.is_err() {
                        drop(allocation);
                        command_family::fail_firmware_dispatch_allocation(uart, state, command_tag)
                            .await;
                        return;
                    }
                    let mut command = allocation.expect("checked internal command allocation");
                    // InternalValue owns a stable allocation until this inline await completes,
                    // so pinning its pointee for that lifetime is sound.
                    let command = unsafe { Pin::new_unchecked(&mut *command) };
                    command.await;
                    return;
                }

                if command_family::pre_box_command_class(&cmd)
                    == command_family::PreBoxCommandClass::Metrics
                {
                    // METRICS is a long multi-line response. Keep its sequential line state out
                    // of every ordinary command and explicitly inside the internal heap.
                    let allocation =
                        crate::firmware::psram::InternalValue::try_new_bounded::<1_200, _>(|| {
                            command_dispatch::metrics_command(uart)
                        });
                    if allocation.is_err() {
                        drop(allocation);
                        let _ =
                            uart_write_all(uart, b"METRICS ERR reason=internal_dispatch_alloc\r\n")
                                .await;
                        return;
                    }
                    let mut command = allocation.expect("checked internal metrics allocation");
                    let command = unsafe { Pin::new_unchecked(&mut *command) };
                    command.await;
                    return;
                }

                if command_family::pre_box_command_class(&cmd)
                    == command_family::PreBoxCommandClass::MetricsNet
                {
                    let allocation =
                        crate::firmware::psram::InternalValue::try_new_bounded::<1_200, _>(|| {
                            command_dispatch::metrics_net_command(uart)
                        });
                    if allocation.is_err() {
                        drop(allocation);
                        let _ = uart_write_all(
                            uart,
                            b"METRICSNET ERR reason=internal_dispatch_alloc\r\n",
                        )
                        .await;
                        return;
                    }
                    let mut command = allocation.expect("checked internal metrics allocation");
                    let command = unsafe { Pin::new_unchecked(&mut *command) };
                    command.await;
                    return;
                }

                #[cfg(feature = "ble-foundation")]
                let low_overhead_diagnostic = matches!(
                    cmd,
                    SerialCommand::StackStatus
                        | SerialCommand::AllocatorStatus
                        | SerialCommand::FirmwareAbort
                        | SerialCommand::BlePhase1sStart { .. }
                        | SerialCommand::BlePhase1sStatus
                        | SerialCommand::RadioHandoffAcquire { .. }
                        | SerialCommand::RadioHandoffRelease { .. }
                        | SerialCommand::RadioHandoffStatus
                );
                #[cfg(all(feature = "ble-foundation", feature = "asset-upload-http"))]
                let low_overhead_diagnostic = low_overhead_diagnostic
                    || matches!(
                        cmd,
                        SerialCommand::NetStatus
                            | SerialCommand::StateSet { .. }
                            | SerialCommand::StateDiag { .. }
                            | SerialCommand::NetStart
                            | SerialCommand::NetStop
                            | SerialCommand::NetRecover
                    );
                #[cfg(not(feature = "ble-foundation"))]
                let low_overhead_diagnostic = matches!(
                    cmd,
                    SerialCommand::StackStatus
                        | SerialCommand::AllocatorStatus
                        | SerialCommand::FirmwareAbort
                );
                #[cfg(all(not(feature = "ble-foundation"), feature = "asset-upload-http"))]
                let low_overhead_diagnostic = low_overhead_diagnostic
                    || matches!(
                        cmd,
                        SerialCommand::NetStatus
                            | SerialCommand::StateSet { .. }
                            | SerialCommand::StateDiag { .. }
                            | SerialCommand::NetStart
                            | SerialCommand::NetStop
                            | SerialCommand::NetRecover
                    );
                if low_overhead_diagnostic {
                    // Keep allocator, recovery, and handoff evidence allocation-free while the
                    // network owner measures or changes radio ownership.
                    command_dispatch::run_low_overhead_diagnostic_command(uart, state, cmd).await;
                    return;
                }

                // Preserve the existing inline command ordering for ordinary product commands,
                // but fail before polling rather than falling back to external PSRAM when the
                // internal heap cannot own the bounded command future.
                let allocation =
                    crate::firmware::psram::InternalValue::try_new_bounded::<1_500, _>(|| {
                        command_dispatch::handle_serial_command(uart, state, cmd)
                    });
                if allocation.is_err() {
                    drop(allocation);
                    let _ =
                        uart_write_all(uart, b"CMD ERR reason=internal_dispatch_alloc\r\n").await;
                    return;
                }
                let mut command = allocation.expect("checked internal ordinary command allocation");
                let command = unsafe { Pin::new_unchecked(&mut *command) };
                command.await;
            } else {
                let _ = uart_write_all(uart, b"CMD ERR\r\n").await;
            }
        }
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests;
