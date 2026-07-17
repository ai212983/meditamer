mod command_dispatch;
mod commands;
mod io;
mod labels;
mod line_reader;
mod metrics;
mod parser;
mod queue;
mod status;
mod task_state;

use embassy_futures::yield_now;
use embassy_time::{with_timeout, Duration};

use line_reader::{LineReadEvent, SerialLineReader};
use parser::parse_serial_command;
use task_state::SerialTaskState;

use super::super::{touch::debug_log::uart_write_all, types::SerialUart};

#[embassy_executor::task]
pub(crate) async fn time_sync_task(mut uart: SerialUart) {
    let mut line_reader = SerialLineReader::new();
    let mut rx = [0u8; 128];
    let mut state = SerialTaskState::new();

    state.write_trace_headers(&mut uart).await;

    loop {
        state.drain_runtime_samples(&mut uart).await;

        if let Ok(Ok(read)) =
            with_timeout(Duration::from_millis(10), uart.read_async(&mut rx)).await
        {
            for byte in rx[..read].iter().copied() {
                handle_uart_byte(&mut uart, &mut line_reader, &mut state, byte).await;
            }
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
                command_dispatch::handle_serial_command(uart, state, cmd).await;
            } else {
                let _ = uart_write_all(uart, b"CMD ERR\r\n").await;
            }
        }
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests;
