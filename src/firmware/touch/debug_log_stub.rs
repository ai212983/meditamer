use crate::firmware::types::{SerialUart, TouchEvent};

pub(crate) async fn uart_write_all(_uart: &mut SerialUart, bytes: &[u8]) -> bool {
    crate::write_uart_response(bytes).await;
    true
}

pub(crate) async fn write_touch_event_trace_sample(_uart: &mut SerialUart, _event: TouchEvent) {}
