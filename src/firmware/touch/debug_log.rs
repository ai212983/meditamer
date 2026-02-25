use core::fmt::Write;

use super::super::types::SerialUart;

use super::types::{
    TouchEvent, TouchEventKind, TouchSwipeDirection, TouchTraceSample, TouchWizardSessionEvent,
    TouchWizardSwipeTraceSample,
};

const TOUCH_WIZARD_SESSION_CAPACITY: usize = 128;
const TOUCH_WIZARD_EVENT_CAPACITY: usize = 192;
const TOUCH_WIZARD_TOUCH_SAMPLE_CAPACITY: usize = 768;

pub(crate) struct TouchWizardSessionLog {
    active: bool,
    pending_end_ms: Option<u64>,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    overflow: bool,
    event_overflow: bool,
    touch_sample_overflow: bool,
    samples: heapless::Vec<TouchWizardSwipeTraceSample, TOUCH_WIZARD_SESSION_CAPACITY>,
    events: heapless::Vec<TouchEvent, TOUCH_WIZARD_EVENT_CAPACITY>,
    touch_samples: heapless::Vec<TouchTraceSample, TOUCH_WIZARD_TOUCH_SAMPLE_CAPACITY>,
}

impl TouchWizardSessionLog {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            pending_end_ms: None,
            start_ms: None,
            end_ms: None,
            overflow: false,
            event_overflow: false,
            touch_sample_overflow: false,
            samples: heapless::Vec::new(),
            events: heapless::Vec::new(),
            touch_samples: heapless::Vec::new(),
        }
    }

    pub(crate) fn on_session_event(&mut self, event: TouchWizardSessionEvent) {
        match event {
            TouchWizardSessionEvent::Start { t_ms } => {
                self.samples.clear();
                self.events.clear();
                self.touch_samples.clear();
                self.overflow = false;
                self.event_overflow = false;
                self.touch_sample_overflow = false;
                self.active = true;
                self.pending_end_ms = None;
                self.start_ms = Some(t_ms);
                self.end_ms = None;
            }
            TouchWizardSessionEvent::End { t_ms } => {
                self.pending_end_ms = Some(t_ms);
            }
        }
    }

    pub(crate) fn on_swipe_sample(&mut self, sample: TouchWizardSwipeTraceSample) {
        if self.active && self.samples.push(sample).is_err() {
            self.overflow = true;
            let _ = self.samples.remove(0);
            let _ = self.samples.push(sample);
        }
    }

    pub(crate) fn on_touch_event(&mut self, event: TouchEvent) {
        if self.active && self.events.push(event).is_err() {
            self.event_overflow = true;
            let _ = self.events.remove(0);
            let _ = self.events.push(event);
        }
    }

    pub(crate) fn on_touch_sample(&mut self, sample: TouchTraceSample) {
        if self.active && self.touch_samples.push(sample).is_err() {
            self.touch_sample_overflow = true;
            let _ = self.touch_samples.remove(0);
            let _ = self.touch_samples.push(sample);
        }
    }

    pub(crate) fn settle_pending_end(&mut self) -> bool {
        if let Some(t_ms) = self.pending_end_ms.take() {
            self.active = false;
            self.end_ms = Some(t_ms);
            return true;
        }
        false
    }

    pub(crate) async fn write_dump(&self, uart: &mut SerialUart) {
        let mut summary = heapless::String::<256>::new();
        let _ = write!(
            &mut summary,
            "TOUCH_WIZARD_DUMP BEGIN start_ms={} end_ms={} active={} samples={} overflow={} events={} event_overflow={} touch_samples={} touch_sample_overflow={}\r\n",
            self.start_ms.unwrap_or(0),
            self.end_ms.unwrap_or(0),
            bool_as_u8(self.active),
            self.samples.len(),
            bool_as_u8(self.overflow),
            self.events.len(),
            bool_as_u8(self.event_overflow),
            self.touch_samples.len(),
            bool_as_u8(self.touch_sample_overflow)
        );
        let _ = uart_write_all(uart, summary.as_bytes()).await;
        let _ = uart_write_all(
            uart,
            b"touch_wizard_swipe,ms,case,attempt,expected_dir,expected_speed,verdict,class_dir,start_x,start_y,end_x,end_y,duration_ms,move_count,max_travel_px,release_debounce_ms,dropout_count\r\n",
        )
        .await;
        for sample in &self.samples {
            write_touch_wizard_swipe_trace_sample(uart, *sample).await;
        }
        let _ = uart_write_all(
            uart,
            b"touch_event,ms,kind,x,y,start_x,start_y,duration_ms,count,move_count,max_travel_px,release_debounce_ms,dropout_count\r\n",
        )
        .await;
        for event in &self.events {
            write_touch_event_trace_sample(uart, *event).await;
        }
        let _ = uart_write_all(
            uart,
            b"touch_trace,ms,count,x0,y0,x1,y1,raw0,raw1,raw2,raw3,raw4,raw5,raw6,raw7\r\n",
        )
        .await;
        for sample in &self.touch_samples {
            write_touch_trace_sample(uart, *sample).await;
        }
        let _ = uart_write_all(uart, b"TOUCH_WIZARD_DUMP END\r\n").await;
    }
}

pub(crate) async fn write_touch_wizard_swipe_trace_sample(
    uart: &mut SerialUart,
    sample: TouchWizardSwipeTraceSample,
) {
    let mut line = heapless::String::<256>::new();
    let _ = write!(
        &mut line,
        "touch_wizard_swipe,{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\r\n",
        sample.t_ms,
        sample.case_index,
        sample.attempt,
        wizard_direction_label(sample.expected_direction),
        wizard_speed_label(sample.expected_speed),
        wizard_verdict_label(sample.verdict),
        wizard_direction_label(sample.classified_direction),
        sample.start_x,
        sample.start_y,
        sample.end_x,
        sample.end_y,
        sample.duration_ms,
        sample.move_count,
        sample.max_travel_px,
        sample.release_debounce_ms,
        sample.dropout_count
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(crate) async fn write_touch_event_trace_sample(uart: &mut SerialUart, event: TouchEvent) {
    let mut line = heapless::String::<196>::new();
    let _ = write!(
        &mut line,
        "touch_event,{},{},{},{},{},{},{},{},{},{},{},{}\r\n",
        event.t_ms,
        touch_event_kind_label(event.kind),
        event.x,
        event.y,
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

pub(crate) async fn uart_write_all(uart: &mut SerialUart, mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        match uart.write_async(bytes).await {
            Ok(0) => return false,
            Ok(written) => bytes = &bytes[written..],
            Err(_) => return false,
        }
    }
    true
}

fn wizard_direction_label(direction: u8) -> &'static str {
    match direction {
        0 => "left",
        1 => "right",
        2 => "up",
        3 => "down",
        _ => "na",
    }
}

fn wizard_speed_label(speed: u8) -> &'static str {
    match speed {
        0 => "extra_fast",
        1 => "fast",
        2 => "medium",
        3 => "slow",
        _ => "na",
    }
}

fn wizard_verdict_label(verdict: u8) -> &'static str {
    match verdict {
        0 => "pass",
        1 => "mismatch",
        2 => "release_no_swipe",
        3 => "manual_mark",
        4 => "skip",
        _ => "unknown",
    }
}

fn bool_as_u8(value: bool) -> u8 {
    if value {
        1
    } else {
        0
    }
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
