use core::fmt::Write;

use super::super::types::SerialUart;
use super::types::{TouchEvent, TouchEventKind, TouchSwipeDirection, TouchTraceSample};

pub(crate) async fn write_touch_event_trace_sample(uart: &mut SerialUart, event: TouchEvent) {
    let mut line = heapless::String::<196>::new();
    let _ = write!(
        &mut line,
        "touch_event,{},{},{},{},{},{},{},{},{},{},{},{},{},{}\r\n",
        event.t_ms,
        touch_event_kind_label(event.kind),
        event.x,
        event.y,
        event.contact_x,
        event.contact_y,
        event.start_x,
        event.start_y,
        event.duration_ms,
        event.touch_count,
        event.move_count,
        event.max_travel_px,
        event.release_debounce_ms,
        event.dropout_count
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(crate) async fn write_touch_trace_sample(uart: &mut SerialUart, sample: TouchTraceSample) {
    let mut line = heapless::String::<224>::new();
    let _ = write!(
        &mut line,
        "touch_trace,{},{},{},{},{},{},{:#04x},{:#04x},{:#04x},{:#04x},{:#04x},{:#04x},{:#04x},{:#04x}\r\n",
        sample.t_ms,
        sample.count,
        sample.x0,
        sample.y0,
        sample.x1,
        sample.y1,
        sample.raw[0],
        sample.raw[1],
        sample.raw[2],
        sample.raw[3],
        sample.raw[4],
        sample.raw[5],
        sample.raw[6],
        sample.raw[7]
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(crate) async fn uart_write_all(_uart: &mut SerialUart, bytes: &[u8]) -> bool {
    // UART0 is also the diagnostic sink. Reserve its interrupt-enabled ROM
    // writer for this complete response; competing diagnostics drop and count
    // instead of interleaving or masking radio interrupts.
    crate::write_uart_response(bytes).await;
    true
}

fn touch_event_kind_label(kind: TouchEventKind) -> &'static str {
    match kind {
        TouchEventKind::Down => "down",
        TouchEventKind::Move => "move",
        TouchEventKind::Up => "up",
        TouchEventKind::Tap => "tap",
        TouchEventKind::LongPress => "long_press",
        TouchEventKind::Swipe(TouchSwipeDirection::Left) => "swipe_left",
        TouchEventKind::Swipe(TouchSwipeDirection::Right) => "swipe_right",
        TouchEventKind::Swipe(TouchSwipeDirection::Up) => "swipe_up",
        TouchEventKind::Swipe(TouchSwipeDirection::Down) => "swipe_down",
        TouchEventKind::Cancel => "cancel",
    }
}
