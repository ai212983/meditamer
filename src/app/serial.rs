use core::{fmt::Write, sync::atomic::Ordering};

use embassy_time::{with_timeout, Duration};

use super::{
    config::{
        APP_EVENTS, LAST_MARBLE_REDRAW_MS, MAX_MARBLE_REDRAW_MS, TAP_TRACE_ENABLED,
        TAP_TRACE_SAMPLES, TIMESET_CMD_BUF_LEN, TOUCH_EVENT_TRACE_ENABLED,
        TOUCH_EVENT_TRACE_SAMPLES, TOUCH_TRACE_ENABLED, TOUCH_TRACE_SAMPLES,
    },
    types::{
        AppEvent, SerialUart, TapTraceSample, TimeSyncCommand, TouchEvent, TouchEventKind,
        TouchSwipeDirection, TouchTraceSample,
    },
};

#[derive(Clone, Copy)]
enum SerialCommand {
    TimeSync(TimeSyncCommand),
    Repaint,
    RepaintMarble,
    Metrics,
    SdProbe,
}

#[embassy_executor::task]
pub(crate) async fn time_sync_task(mut uart: SerialUart) {
    let mut line_buf = [0u8; TIMESET_CMD_BUF_LEN];
    let mut line_len = 0usize;
    let mut rx = [0u8; 1];

    if TAP_TRACE_ENABLED {
        let _ = uart
            .write_async(
                b"tap_trace,ms,tap_src,seq,cand,csrc,state,reject,score,window,cooldown,jerk,veto,gyro,int1,int2,pgood,batt_pct,gx,gy,gz,ax,ay,az\r\n",
            )
            .await;
    }
    if TOUCH_TRACE_ENABLED {
        let _ = uart
            .write_async(
                b"touch_trace,ms,count,x0,y0,x1,y1,raw0,raw1,raw2,raw3,raw4,raw5,raw6,raw7\r\n",
            )
            .await;
    }
    if TOUCH_EVENT_TRACE_ENABLED {
        let _ = uart
            .write_async(b"touch_event,ms,kind,x,y,start_x,start_y,duration_ms,count\r\n")
            .await;
    }

    loop {
        if TOUCH_EVENT_TRACE_ENABLED {
            while let Ok(event) = TOUCH_EVENT_TRACE_SAMPLES.try_receive() {
                write_touch_event_trace_sample(&mut uart, event).await;
            }
        }

        if TOUCH_TRACE_ENABLED {
            while let Ok(sample) = TOUCH_TRACE_SAMPLES.try_receive() {
                write_touch_trace_sample(&mut uart, sample).await;
            }
        }

        if TAP_TRACE_ENABLED {
            while let Ok(sample) = TAP_TRACE_SAMPLES.try_receive() {
                write_tap_trace_sample(&mut uart, sample).await;
            }
        }

        match with_timeout(Duration::from_millis(10), uart.read_async(&mut rx)).await {
            Ok(Ok(1)) => {
                let byte = rx[0];
                if byte == b'\r' || byte == b'\n' {
                    if line_len == 0 {
                        continue;
                    }
                    if let Some(cmd) = parse_serial_command(&line_buf[..line_len]) {
                        match cmd {
                            SerialCommand::TimeSync(cmd) => {
                                if APP_EVENTS.try_send(AppEvent::TimeSync(cmd)).is_ok() {
                                    let _ = uart.write_async(b"TIMESET OK\r\n").await;
                                } else {
                                    let _ = uart.write_async(b"TIMESET BUSY\r\n").await;
                                }
                            }
                            SerialCommand::Repaint => {
                                if APP_EVENTS.try_send(AppEvent::ForceRepaint).is_ok() {
                                    let _ = uart.write_async(b"REPAINT OK\r\n").await;
                                } else {
                                    let _ = uart.write_async(b"REPAINT BUSY\r\n").await;
                                }
                            }
                            SerialCommand::RepaintMarble => {
                                if APP_EVENTS.try_send(AppEvent::ForceMarbleRepaint).is_ok() {
                                    let _ = uart.write_async(b"REPAINT_MARBLE OK\r\n").await;
                                } else {
                                    let _ = uart.write_async(b"REPAINT_MARBLE BUSY\r\n").await;
                                }
                            }
                            SerialCommand::Metrics => {
                                let last_ms = LAST_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
                                let max_ms = MAX_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
                                let mut line = heapless::String::<96>::new();
                                let _ = write!(
                                    &mut line,
                                    "METRICS MARBLE_REDRAW_MS={} MAX_MS={}\r\n",
                                    last_ms, max_ms
                                );
                                let _ = uart.write_async(line.as_bytes()).await;
                            }
                            SerialCommand::SdProbe => {
                                if APP_EVENTS.try_send(AppEvent::SdProbe).is_ok() {
                                    let _ = uart.write_async(b"SDPROBE OK\r\n").await;
                                } else {
                                    let _ = uart.write_async(b"SDPROBE BUSY\r\n").await;
                                }
                            }
                        }
                    } else {
                        let _ = uart.write_async(b"CMD ERR\r\n").await;
                    }
                    line_len = 0;
                } else if line_len < line_buf.len() {
                    line_buf[line_len] = byte;
                    line_len += 1;
                } else {
                    line_len = 0;
                }
            }
            _ => {}
        }
    }
}

async fn write_tap_trace_sample(uart: &mut SerialUart, sample: TapTraceSample) {
    let mut line = heapless::String::<256>::new();
    let _ = write!(
        &mut line,
        "tap_trace,{},{:#04x},{},{},{:#04x},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\r\n",
        sample.t_ms,
        sample.tap_src,
        sample.seq_count,
        sample.tap_candidate,
        sample.cand_src,
        sample.state_id,
        sample.reject_reason,
        sample.candidate_score,
        sample.window_ms,
        sample.cooldown_active,
        sample.jerk_l1,
        sample.motion_veto,
        sample.gyro_l1,
        sample.int1,
        sample.int2,
        sample.power_good,
        sample.battery_percent,
        sample.gx,
        sample.gy,
        sample.gz,
        sample.ax,
        sample.ay,
        sample.az
    );
    let _ = uart.write_async(line.as_bytes()).await;
}

async fn write_touch_trace_sample(uart: &mut SerialUart, sample: TouchTraceSample) {
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
    let _ = uart.write_async(line.as_bytes()).await;
}

async fn write_touch_event_trace_sample(uart: &mut SerialUart, event: TouchEvent) {
    let mut line = heapless::String::<144>::new();
    let _ = write!(
        &mut line,
        "touch_event,{},{},{},{},{},{},{},{}\r\n",
        event.t_ms,
        touch_event_kind_label(event.kind),
        event.x,
        event.y,
        event.start_x,
        event.start_y,
        event.duration_ms,
        event.touch_count
    );
    let _ = uart.write_async(line.as_bytes()).await;
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

fn parse_serial_command(line: &[u8]) -> Option<SerialCommand> {
    if parse_repaint_marble_command(line) {
        return Some(SerialCommand::RepaintMarble);
    }
    if parse_repaint_command(line) {
        return Some(SerialCommand::Repaint);
    }
    if parse_metrics_command(line) {
        return Some(SerialCommand::Metrics);
    }
    if parse_sdprobe_command(line) {
        return Some(SerialCommand::SdProbe);
    }

    parse_timeset_command(line).map(SerialCommand::TimeSync)
}

fn parse_timeset_command(line: &[u8]) -> Option<TimeSyncCommand> {
    let cmd_idx = find_subslice(line, b"TIMESET")?;
    let mut i = cmd_idx + b"TIMESET".len();
    let len = line.len();

    while i < len && line[i].is_ascii_whitespace() {
        i += 1;
    }
    let (unix_epoch_utc_seconds, next_i) = parse_u64_ascii(line, i)?;
    i = next_i;
    while i < len && line[i].is_ascii_whitespace() {
        i += 1;
    }
    let (tz_offset_minutes, next_i) = parse_i32_ascii(line, i)?;
    i = next_i;
    while i < len && line[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != len {
        return None;
    }
    if !(-720..=840).contains(&tz_offset_minutes) {
        return None;
    }

    Some(TimeSyncCommand {
        unix_epoch_utc_seconds,
        tz_offset_minutes,
    })
}

fn parse_repaint_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"REPAINT" || cmd == b"REFRESH"
}

fn parse_repaint_marble_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"REPAINT_MARBLE" || cmd == b"MARBLE"
}

fn parse_metrics_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"METRICS" || cmd == b"PERF"
}

fn parse_sdprobe_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"SDPROBE"
}

fn trim_ascii_whitespace(line: &[u8]) -> &[u8] {
    let mut start = 0usize;
    let mut end = line.len();
    while start < end && line[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && line[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &line[start..end]
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&idx| &haystack[idx..idx + needle.len()] == needle)
}

fn parse_u64_ascii(bytes: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        value = value
            .checked_mul(10)?
            .checked_add((bytes[i] - b'0') as u64)?;
        i += 1;
    }
    if i == start {
        None
    } else {
        Some((value, i))
    }
}

fn parse_i32_ascii(bytes: &[u8], i: usize) -> Option<(i32, usize)> {
    if i >= bytes.len() {
        return None;
    }
    let mut idx = i;
    let mut sign = 1i64;
    if bytes[idx] == b'-' {
        sign = -1;
        idx += 1;
    } else if bytes[idx] == b'+' {
        idx += 1;
    }

    let (unsigned, next_idx) = parse_u64_ascii(bytes, idx)?;
    let signed = sign.checked_mul(unsigned as i64)?;
    if signed < i32::MIN as i64 || signed > i32::MAX as i64 {
        return None;
    }
    Some((signed as i32, next_idx))
}
