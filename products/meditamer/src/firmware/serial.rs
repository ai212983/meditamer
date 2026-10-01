extern crate alloc;

#[embassy_executor::task]
pub async fn console_task() {
    embassy_futures::join::join(console::drain_logs(), bulk::run()).await;
}

mod bulk;
mod byte_dispatch;
mod command_dispatch;
mod command_family;
mod commands;
mod io;
mod labels;
mod line_reader;
mod metrics;
mod parser;
mod queue;
mod status;
mod task_state;
mod time_dispatch;

use embassy_futures::{
    select::{select, Either},
    yield_now,
};
use embassy_time::{Duration, Instant, Timer};

use line_reader::{LineReadEvent, SerialLineReader};
use task_state::SerialTaskState;

use super::{
    touch::debug_log::uart_write_all,
    types::{RtcI2cDevice, SerialUart, SerialWriter},
};

#[embassy_executor::task]
pub async fn serial_task(mut rx_uart: SerialUart, rtc_i2c: RtcI2cDevice) {
    let mut uart = SerialWriter;
    let mut line_reader = SerialLineReader::new();
    let mut rx = [0u8; 128];
    let mut state = SerialTaskState::new(rtc_i2c);

    state.write_trace_headers(&mut uart).await;
    time_dispatch::open_boot_session(&mut uart, &mut state).await;

    loop {
        state.drain_runtime_samples(&mut uart).await;
        time_dispatch::process_wall_clock_requests(&mut state).await;
        time_dispatch::poll_wall_clock_session(&mut uart, &mut state).await;

        let remaining = state
            .wall_clock_mut()
            .timeout_remaining_ms(Instant::now().as_millis() as u32);
        let deadline = async {
            match remaining {
                Some(ms) => Timer::after(Duration::from_millis(u64::from(ms))).await,
                None => core::future::pending::<()>().await,
            }
        };
        let work = select(
            core::future::poll_fn(SerialTaskState::poll_runtime_work),
            deadline,
        );
        match select(rx_uart.read_async(&mut rx), work).await {
            Either::First(Ok(read)) => {
                for byte in rx[..read].iter().copied() {
                    handle_uart_byte(&mut uart, &mut line_reader, &mut state, byte).await;
                }
            }
            Either::First(Err(error)) => {
                line_reader.discard_until_line_end();
                console::println!("SERIAL_RX status=error reason={error:?}");
            }
            Either::Second(_) => {}
        }
        yield_now().await;
    }
}

async fn handle_uart_byte(
    uart: &mut SerialWriter,
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
            // Parse and classify synchronously before the first await so
            // neither the line borrow nor a duplicate command is retained
            // in a nested async frame.
            let Some(cmd) = parser::parse_serial_command(line) else {
                let _ = uart_write_all(uart, b"CMD ERR\r\n").await;
                return;
            };
            match byte_dispatch::route_for_command(&cmd) {
                byte_dispatch::DispatchRoute::ObservationFixture => {
                    if let commands::SerialCommand::ObservationFixture(command) = cmd {
                        // Bounded enqueue/log only: no command allocation or wait
                        // on UI acquisition, and QUEUED precedes any UI result.
                        command_dispatch::queue_observation_fixture(command);
                    }
                }
                byte_dispatch::DispatchRoute::ObservationPanelCycle => {
                    if let commands::SerialCommand::ObservationPanelCycle(command) = cmd {
                        crate::firmware::observation_fixture::enqueue_panel(command);
                    }
                }
                byte_dispatch::DispatchRoute::Firmware => {
                    let command_tag = command_family::firmware_command_tag(&cmd);
                    // Firmware update commands retain large stream/verification
                    // state and may execute while the flash cache is disabled.
                    // Keep that family in its own internal allocation so its
                    // maximum future size does not inflate every ordinary
                    // serial command, and never place it in external PSRAM.
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
                    let command = command.pin_mut();
                    command.await;
                }
                byte_dispatch::DispatchRoute::MetricsGeneral => {
                    if !bulk::request_metrics(bulk::MetricsKind::General) {
                        let _ = uart_write_all(
                            uart,
                            byte_dispatch::metrics_busy_line(
                                byte_dispatch::DispatchRoute::MetricsGeneral,
                            ),
                        )
                        .await;
                    }
                }
                byte_dispatch::DispatchRoute::MetricsNetwork => {
                    if !bulk::request_metrics(bulk::MetricsKind::Network) {
                        let _ = uart_write_all(
                            uart,
                            byte_dispatch::metrics_busy_line(
                                byte_dispatch::DispatchRoute::MetricsNetwork,
                            ),
                        )
                        .await;
                    }
                }
                #[cfg(feature = "firmware-trace")]
                byte_dispatch::DispatchRoute::Trace => {
                    let commands::SerialCommand::Trace { operation } = cmd else {
                        let _ = uart_write_all(uart, b"CMD ERR\r\n").await;
                        return;
                    };
                    // Trace export/self-test owns larger bounded formatting and
                    // coordination state. Isolate it from every ordinary command.
                    let allocation =
                        crate::firmware::psram::InternalValue::try_new_bounded::<4_096, _>(|| {
                            command_dispatch::handle_trace_command(uart, operation)
                        });
                    if allocation.is_err() {
                        drop(allocation);
                        let _ =
                            uart_write_all(uart, b"TRACE ERROR reason=internal_dispatch_alloc\r\n")
                                .await;
                        return;
                    }
                    let mut command = allocation.expect("checked trace command allocation");
                    command.pin_mut().await;
                }
                byte_dispatch::DispatchRoute::LowOverhead => {
                    // Keep allocator, recovery, and handoff evidence
                    // allocation-free while the network owner measures or
                    // changes radio ownership.
                    command_dispatch::run_low_overhead_diagnostic_command(uart, state, cmd).await;
                }
                byte_dispatch::DispatchRoute::Ordinary => {
                    // Preserve the existing inline command ordering for ordinary
                    // product commands, but fail before polling rather than
                    // falling back to external PSRAM when the internal heap
                    // cannot own the bounded command future.
                    let allocation =
                        crate::firmware::psram::InternalValue::try_new_bounded::<2_048, _>(|| {
                            command_dispatch::handle_serial_command(uart, state, cmd)
                        });
                    if allocation.is_err() {
                        drop(allocation);
                        let _ = uart_write_all(uart, b"CMD ERR reason=internal_dispatch_alloc\r\n")
                            .await;
                        return;
                    }
                    let mut command =
                        allocation.expect("checked internal ordinary command allocation");
                    let command = command.pin_mut();
                    command.await;
                }
            }
        }
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests;
