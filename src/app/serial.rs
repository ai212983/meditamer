use core::{fmt::Write, sync::atomic::Ordering};

use embassy_time::{with_timeout, Duration, Instant, Timer};

use super::{
    config::{
        APP_EVENTS, LAST_MARBLE_REDRAW_MS, MAX_MARBLE_REDRAW_MS, SD_REQUESTS, SD_RESULTS,
        TAP_TRACE_ENABLED, TAP_TRACE_SAMPLES, TIMESET_CMD_BUF_LEN,
    },
    touch::{
        config::{
            TOUCH_EVENT_TRACE_ENABLED, TOUCH_EVENT_TRACE_SAMPLES, TOUCH_TRACE_ENABLED,
            TOUCH_TRACE_SAMPLES, TOUCH_WIZARD_RAW_TRACE_SAMPLES, TOUCH_WIZARD_SESSION_EVENTS,
            TOUCH_WIZARD_SWIPE_TRACE_SAMPLES, TOUCH_WIZARD_TRACE_ENABLED,
        },
        debug_log::{
            uart_write_all, write_touch_event_trace_sample, write_touch_trace_sample,
            write_touch_wizard_swipe_trace_sample, TouchWizardSessionLog,
        },
    },
    types::{
        AppEvent, SdCommand, SdCommandKind, SdRequest, SdResult, SdResultCode, SerialUart,
        TapTraceSample, TimeSyncCommand, SD_PATH_MAX, SD_WRITE_MAX,
    },
};

#[derive(Clone, Copy)]
enum SerialCommand {
    TimeSync(TimeSyncCommand),
    TouchWizard,
    TouchWizardDump,
    Repaint,
    RepaintMarble,
    Metrics,
    Probe,
    RwVerify {
        lba: u32,
    },
    FatList {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRead {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatWrite {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        data: [u8; SD_WRITE_MAX],
        data_len: u16,
    },
    FatStat {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatMkdir {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRemove {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRename {
        src_path: [u8; SD_PATH_MAX],
        src_path_len: u8,
        dst_path: [u8; SD_PATH_MAX],
        dst_path_len: u8,
    },
    FatAppend {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        data: [u8; SD_WRITE_MAX],
        data_len: u16,
    },
    FatTruncate {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        size: u32,
    },
    SdWait {
        target: SdWaitTarget,
        timeout_ms: u32,
    },
}

#[derive(Clone, Copy)]
enum SdWaitTarget {
    Next,
    Last,
    Id(u32),
}

const APP_EVENT_ENQUEUE_RETRY_MS: u64 = 25;
// Absorb short SD/FAT bursts without requiring host-side pacing.
const APP_EVENT_ENQUEUE_MAX_RETRIES: u8 = 240;
const SD_RESULT_CACHE_CAP: usize = 16;
const SDWAIT_DEFAULT_TIMEOUT_MS: u32 = 10_000;

#[embassy_executor::task]
pub(crate) async fn time_sync_task(mut uart: SerialUart) {
    let mut line_buf = [0u8; TIMESET_CMD_BUF_LEN];
    let mut line_len = 0usize;
    let mut rx = [0u8; 1];
    let mut touch_wizard_log = TouchWizardSessionLog::new();
    let mut next_sd_request_id = 1u32;
    let mut last_sd_request_id: Option<u32> = None;
    let mut sd_result_cache = heapless::Vec::<SdResult, SD_RESULT_CACHE_CAP>::new();

    if TAP_TRACE_ENABLED {
        let _ = uart_write_all(
            &mut uart,
            b"tap_trace,ms,tap_src,seq,cand,csrc,state,reject,score,window,cooldown,jerk,veto,gyro,int1,int2,pgood,batt_pct,gx,gy,gz,ax,ay,az\r\n",
        )
        .await;
    }
    if TOUCH_TRACE_ENABLED {
        let _ = uart_write_all(
            &mut uart,
            b"touch_trace,ms,count,x0,y0,x1,y1,raw0,raw1,raw2,raw3,raw4,raw5,raw6,raw7\r\n",
        )
        .await;
    }
    if TOUCH_EVENT_TRACE_ENABLED {
        let _ = uart_write_all(
            &mut uart,
            b"touch_event,ms,kind,x,y,start_x,start_y,duration_ms,count,move_count,max_travel_px,release_debounce_ms,dropout_count\r\n",
        )
        .await;
    }
    if TOUCH_WIZARD_TRACE_ENABLED {
        let _ = uart_write_all(
            &mut uart,
            b"touch_wizard_swipe,ms,case,attempt,expected_dir,expected_speed,verdict,class_dir,start_x,start_y,end_x,end_y,duration_ms,move_count,max_travel_px,release_debounce_ms,dropout_count\r\n",
        )
        .await;
    }

    loop {
        while let Ok(session_event) = TOUCH_WIZARD_SESSION_EVENTS.try_receive() {
            touch_wizard_log.on_session_event(session_event);
        }

        if TOUCH_EVENT_TRACE_ENABLED {
            while let Ok(event) = TOUCH_EVENT_TRACE_SAMPLES.try_receive() {
                touch_wizard_log.on_touch_event(event);
                write_touch_event_trace_sample(&mut uart, event).await;
            }
        }

        while let Ok(sample) = TOUCH_WIZARD_SWIPE_TRACE_SAMPLES.try_receive() {
            touch_wizard_log.on_swipe_sample(sample);
            if TOUCH_WIZARD_TRACE_ENABLED {
                write_touch_wizard_swipe_trace_sample(&mut uart, sample).await;
            }
        }
        while let Ok(sample) = TOUCH_WIZARD_RAW_TRACE_SAMPLES.try_receive() {
            touch_wizard_log.on_touch_sample(sample);
        }

        if touch_wizard_log.settle_pending_end() {
            touch_wizard_log.write_dump(&mut uart).await;
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

        while let Ok(result) = SD_RESULTS.try_receive() {
            cache_sd_result(&mut sd_result_cache, result);
            write_sd_result(&mut uart, result).await;
        }

        if let Ok(Ok(1)) = with_timeout(Duration::from_millis(10), uart.read_async(&mut rx)).await {
            let byte = rx[0];
            if byte == b'\r' || byte == b'\n' {
                if line_len == 0 {
                    continue;
                }
                if let Some(cmd) = parse_serial_command(&line_buf[..line_len]) {
                    match cmd {
                        SerialCommand::TouchWizardDump => {
                            touch_wizard_log.write_dump(&mut uart).await;
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
                            let _ = uart_write_all(&mut uart, line.as_bytes()).await;
                        }
                        SerialCommand::SdWait { target, timeout_ms } => {
                            run_sdwait_command(
                                &mut uart,
                                &mut sd_result_cache,
                                last_sd_request_id,
                                target,
                                timeout_ms,
                            )
                            .await;
                        }
                        _ => {
                            let (app_event, sd_command, ok_response, busy_response) =
                                serial_command_event_and_responses(cmd);
                            let mut sd_request_meta: Option<(u32, SdCommand)> = None;
                            let queued = if let Some(event) = app_event {
                                enqueue_app_event_with_retry(event).await
                            } else if let Some(command) = sd_command {
                                let request_id = next_sd_request_id;
                                next_sd_request_id = next_sd_request_id.wrapping_add(1);
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
                                    last_sd_request_id = Some(request_id);
                                    write_sd_request_queued(&mut uart, request_id, command).await;
                                }
                            } else {
                                let _ = uart.write_async(busy_response).await;
                            }
                        }
                    }
                } else {
                    let _ = uart_write_all(&mut uart, b"CMD ERR\r\n").await;
                }
                line_len = 0;
            } else if line_len < line_buf.len() {
                line_buf[line_len] = byte;
                line_len += 1;
            } else {
                line_len = 0;
            }
        }
    }
}

async fn enqueue_app_event_with_retry(event: AppEvent) -> bool {
    for attempt in 0..=APP_EVENT_ENQUEUE_MAX_RETRIES {
        if APP_EVENTS.try_send(event).is_ok() {
            return true;
        }
        if attempt == APP_EVENT_ENQUEUE_MAX_RETRIES {
            break;
        }
        Timer::after_millis(APP_EVENT_ENQUEUE_RETRY_MS).await;
    }
    false
}

async fn enqueue_sd_request_with_retry(request: SdRequest) -> bool {
    for attempt in 0..=APP_EVENT_ENQUEUE_MAX_RETRIES {
        if SD_REQUESTS.try_send(request).is_ok() {
            return true;
        }
        if attempt == APP_EVENT_ENQUEUE_MAX_RETRIES {
            break;
        }
        Timer::after_millis(APP_EVENT_ENQUEUE_RETRY_MS).await;
    }
    false
}

fn serial_command_event_and_responses(
    cmd: SerialCommand,
) -> (
    Option<AppEvent>,
    Option<SdCommand>,
    &'static [u8],
    &'static [u8],
) {
    match cmd {
        SerialCommand::TimeSync(cmd) => (
            Some(AppEvent::TimeSync(cmd)),
            None,
            b"TIMESET OK\r\n",
            b"TIMESET BUSY\r\n",
        ),
        SerialCommand::TouchWizard => (
            Some(AppEvent::StartTouchCalibrationWizard),
            None,
            b"TOUCH_WIZARD OK\r\n",
            b"TOUCH_WIZARD BUSY\r\n",
        ),
        SerialCommand::Repaint => (
            Some(AppEvent::ForceRepaint),
            None,
            b"REPAINT OK\r\n",
            b"REPAINT BUSY\r\n",
        ),
        SerialCommand::RepaintMarble => (
            Some(AppEvent::ForceMarbleRepaint),
            None,
            b"REPAINT_MARBLE OK\r\n",
            b"REPAINT_MARBLE BUSY\r\n",
        ),
        SerialCommand::Probe => (
            None,
            Some(SdCommand::Probe),
            b"SDPROBE OK\r\n",
            b"SDPROBE BUSY\r\n",
        ),
        SerialCommand::RwVerify { lba } => (
            None,
            Some(SdCommand::RwVerify { lba }),
            b"SDRWVERIFY OK\r\n",
            b"SDRWVERIFY BUSY\r\n",
        ),
        SerialCommand::FatList { path, path_len } => (
            None,
            Some(SdCommand::FatList { path, path_len }),
            b"SDFATLS OK\r\n",
            b"SDFATLS BUSY\r\n",
        ),
        SerialCommand::FatRead { path, path_len } => (
            None,
            Some(SdCommand::FatRead { path, path_len }),
            b"SDFATREAD OK\r\n",
            b"SDFATREAD BUSY\r\n",
        ),
        SerialCommand::FatWrite {
            path,
            path_len,
            data,
            data_len,
        } => (
            None,
            Some(SdCommand::FatWrite {
                path,
                path_len,
                data,
                data_len,
            }),
            b"SDFATWRITE OK\r\n",
            b"SDFATWRITE BUSY\r\n",
        ),
        SerialCommand::FatStat { path, path_len } => (
            None,
            Some(SdCommand::FatStat { path, path_len }),
            b"SDFATSTAT OK\r\n",
            b"SDFATSTAT BUSY\r\n",
        ),
        SerialCommand::FatMkdir { path, path_len } => (
            None,
            Some(SdCommand::FatMkdir { path, path_len }),
            b"SDFATMKDIR OK\r\n",
            b"SDFATMKDIR BUSY\r\n",
        ),
        SerialCommand::FatRemove { path, path_len } => (
            None,
            Some(SdCommand::FatRemove { path, path_len }),
            b"SDFATRM OK\r\n",
            b"SDFATRM BUSY\r\n",
        ),
        SerialCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        } => (
            None,
            Some(SdCommand::FatRename {
                src_path,
                src_path_len,
                dst_path,
                dst_path_len,
            }),
            b"SDFATREN OK\r\n",
            b"SDFATREN BUSY\r\n",
        ),
        SerialCommand::FatAppend {
            path,
            path_len,
            data,
            data_len,
        } => (
            None,
            Some(SdCommand::FatAppend {
                path,
                path_len,
                data,
                data_len,
            }),
            b"SDFATAPPEND OK\r\n",
            b"SDFATAPPEND BUSY\r\n",
        ),
        SerialCommand::FatTruncate {
            path,
            path_len,
            size,
        } => (
            None,
            Some(SdCommand::FatTruncate {
                path,
                path_len,
                size,
            }),
            b"SDFATTRUNC OK\r\n",
            b"SDFATTRUNC BUSY\r\n",
        ),
        SerialCommand::TouchWizardDump => {
            unreachable!("touch wizard dump command is handled inline")
        }
        SerialCommand::Metrics => unreachable!("metrics command is handled inline"),
        SerialCommand::SdWait { .. } => unreachable!("sdwait command is handled inline"),
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
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

async fn write_sd_request_queued(uart: &mut SerialUart, request_id: u32, command: SdCommand) {
    let mut line = heapless::String::<96>::new();
    let _ = write!(
        &mut line,
        "SDREQ id={} op={}\r\n",
        request_id,
        sd_command_label(command)
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

async fn write_sd_result(uart: &mut SerialUart, result: SdResult) {
    let mut line = heapless::String::<128>::new();
    let _ = write!(
        &mut line,
        "SDDONE id={} op={} status={} code={} attempts={} dur_ms={}\r\n",
        result.id,
        sd_result_kind_label(result.kind),
        if result.ok { "ok" } else { "error" },
        sd_result_code_label(result.code),
        result.attempts,
        result.duration_ms
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

fn cache_sd_result(cache: &mut heapless::Vec<SdResult, SD_RESULT_CACHE_CAP>, result: SdResult) {
    if cache.push(result).is_err() {
        let _ = cache.remove(0);
        let _ = cache.push(result);
    }
}

async fn run_sdwait_command(
    uart: &mut SerialUart,
    sd_result_cache: &mut heapless::Vec<SdResult, SD_RESULT_CACHE_CAP>,
    last_sd_request_id: Option<u32>,
    target: SdWaitTarget,
    timeout_ms: u32,
) {
    let wait_id = match target {
        SdWaitTarget::Next => None,
        SdWaitTarget::Last => {
            let Some(id) = last_sd_request_id else {
                let _ = uart_write_all(uart, b"SDWAIT ERR reason=no_last_request\r\n").await;
                return;
            };
            Some(id)
        }
        SdWaitTarget::Id(id) => Some(id),
    };

    if let Some(id) = wait_id {
        if let Some(result) = sd_result_cache
            .iter()
            .rev()
            .find(|result| result.id == id)
            .copied()
        {
            write_sdwait_done(uart, target, wait_id, result).await;
            return;
        }
    }

    let start = Instant::now();
    loop {
        let elapsed_ms = Instant::now().saturating_duration_since(start).as_millis();
        if elapsed_ms >= timeout_ms as u64 {
            write_sdwait_timeout(uart, target, wait_id, timeout_ms).await;
            return;
        }

        let remaining_ms = (timeout_ms as u64).saturating_sub(elapsed_ms).max(1);
        match with_timeout(Duration::from_millis(remaining_ms), SD_RESULTS.receive()).await {
            Ok(result) => {
                cache_sd_result(sd_result_cache, result);
                write_sd_result(uart, result).await;
                if wait_id.map(|id| id == result.id).unwrap_or(true) {
                    write_sdwait_done(uart, target, wait_id, result).await;
                    return;
                }
            }
            Err(_) => {
                write_sdwait_timeout(uart, target, wait_id, timeout_ms).await;
                return;
            }
        }
    }
}

async fn write_sdwait_done(
    uart: &mut SerialUart,
    target: SdWaitTarget,
    wait_id: Option<u32>,
    result: SdResult,
) {
    let mut line = heapless::String::<192>::new();
    let _ = write!(
        &mut line,
        "SDWAIT DONE target={} ",
        sdwait_target_label(target)
    );
    if let Some(wait_id) = wait_id {
        let _ = write!(&mut line, "wait_id={} ", wait_id);
    }
    let _ = write!(
        &mut line,
        "id={} op={} status={} code={} attempts={} dur_ms={}\r\n",
        result.id,
        sd_result_kind_label(result.kind),
        if result.ok { "ok" } else { "error" },
        sd_result_code_label(result.code),
        result.attempts,
        result.duration_ms
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

async fn write_sdwait_timeout(
    uart: &mut SerialUart,
    target: SdWaitTarget,
    wait_id: Option<u32>,
    timeout_ms: u32,
) {
    let mut line = heapless::String::<112>::new();
    let _ = write!(
        &mut line,
        "SDWAIT TIMEOUT target={} ",
        sdwait_target_label(target)
    );
    if let Some(wait_id) = wait_id {
        let _ = write!(&mut line, "wait_id={} ", wait_id);
    }
    let _ = write!(&mut line, "timeout_ms={}\r\n", timeout_ms);
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

fn sd_command_label(command: SdCommand) -> &'static str {
    match command {
        SdCommand::Probe => "probe",
        SdCommand::RwVerify { .. } => "rw_verify",
        SdCommand::FatList { .. } => "fat_ls",
        SdCommand::FatRead { .. } => "fat_read",
        SdCommand::FatWrite { .. } => "fat_write",
        SdCommand::FatStat { .. } => "fat_stat",
        SdCommand::FatMkdir { .. } => "fat_mkdir",
        SdCommand::FatRemove { .. } => "fat_rm",
        SdCommand::FatRename { .. } => "fat_ren",
        SdCommand::FatAppend { .. } => "fat_append",
        SdCommand::FatTruncate { .. } => "fat_trunc",
    }
}

fn sd_result_kind_label(kind: SdCommandKind) -> &'static str {
    match kind {
        SdCommandKind::Probe => "probe",
        SdCommandKind::RwVerify => "rw_verify",
        SdCommandKind::FatList => "fat_ls",
        SdCommandKind::FatRead => "fat_read",
        SdCommandKind::FatWrite => "fat_write",
        SdCommandKind::FatStat => "fat_stat",
        SdCommandKind::FatMkdir => "fat_mkdir",
        SdCommandKind::FatRemove => "fat_rm",
        SdCommandKind::FatRename => "fat_ren",
        SdCommandKind::FatAppend => "fat_append",
        SdCommandKind::FatTruncate => "fat_trunc",
    }
}

fn sdwait_target_label(target: SdWaitTarget) -> &'static str {
    match target {
        SdWaitTarget::Next => "next",
        SdWaitTarget::Last => "last",
        SdWaitTarget::Id(_) => "id",
    }
}

fn sd_result_code_label(code: SdResultCode) -> &'static str {
    match code {
        SdResultCode::Ok => "ok",
        SdResultCode::PowerOnFailed => "power_on_failed",
        SdResultCode::InitFailed => "init_failed",
        SdResultCode::InvalidPath => "invalid_path",
        SdResultCode::NotFound => "not_found",
        SdResultCode::VerifyMismatch => "verify_mismatch",
        SdResultCode::PowerOffFailed => "power_off_failed",
        SdResultCode::OperationFailed => "operation_failed",
        SdResultCode::RefusedLba0 => "refused_lba0",
    }
}

fn parse_serial_command(line: &[u8]) -> Option<SerialCommand> {
    if parse_repaint_marble_command(line) {
        return Some(SerialCommand::RepaintMarble);
    }
    if parse_repaint_command(line) {
        return Some(SerialCommand::Repaint);
    }
    if parse_touch_wizard_dump_command(line) {
        return Some(SerialCommand::TouchWizardDump);
    }
    if parse_touch_wizard_command(line) {
        return Some(SerialCommand::TouchWizard);
    }
    if parse_metrics_command(line) {
        return Some(SerialCommand::Metrics);
    }
    if parse_sdprobe_command(line) {
        return Some(SerialCommand::Probe);
    }
    if let Some((target, timeout_ms)) = parse_sdwait_command(line) {
        return Some(SerialCommand::SdWait { target, timeout_ms });
    }
    if let Some(lba) = parse_sdrwverify_command(line) {
        return Some(SerialCommand::RwVerify { lba });
    }
    if let Some((path, path_len)) = parse_sdfatls_command(line) {
        return Some(SerialCommand::FatList { path, path_len });
    }
    if let Some((path, path_len)) = parse_sdfatread_command(line) {
        return Some(SerialCommand::FatRead { path, path_len });
    }
    if let Some((path, path_len, data, data_len)) = parse_sdfatwrite_command(line) {
        return Some(SerialCommand::FatWrite {
            path,
            path_len,
            data,
            data_len,
        });
    }
    if let Some((path, path_len)) = parse_sdfatstat_command(line) {
        return Some(SerialCommand::FatStat { path, path_len });
    }
    if let Some((path, path_len)) = parse_sdfatmkdir_command(line) {
        return Some(SerialCommand::FatMkdir { path, path_len });
    }
    if let Some((path, path_len)) = parse_sdfatrm_command(line) {
        return Some(SerialCommand::FatRemove { path, path_len });
    }
    if let Some((src_path, src_path_len, dst_path, dst_path_len)) = parse_sdfatren_command(line) {
        return Some(SerialCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        });
    }
    if let Some((path, path_len, data, data_len)) = parse_sdfatappend_command(line) {
        return Some(SerialCommand::FatAppend {
            path,
            path_len,
            data,
            data_len,
        });
    }
    if let Some((path, path_len, size)) = parse_sdfattrunc_command(line) {
        return Some(SerialCommand::FatTruncate {
            path,
            path_len,
            size,
        });
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

fn parse_touch_wizard_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"TOUCH_WIZARD" || cmd == b"TOUCH_CAL" || cmd == b"CAL_TOUCH"
}

fn parse_touch_wizard_dump_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"TOUCH_WIZARD_DUMP" || cmd == b"TOUCH_DUMP" || cmd == b"WIZARD_DUMP"
}

fn parse_sdprobe_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"SDPROBE"
}

fn parse_sdwait_command(line: &[u8]) -> Option<(SdWaitTarget, u32)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDWAIT";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return Some((SdWaitTarget::Next, SDWAIT_DEFAULT_TIMEOUT_MS));
    }

    let token_start = i;
    while i < trimmed.len() && !trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let token = &trimmed[token_start..i];
    let target = if token == b"LAST" {
        SdWaitTarget::Last
    } else if token == b"NEXT" {
        SdWaitTarget::Next
    } else {
        let (id, next_i) = parse_u64_ascii(trimmed, token_start)?;
        if next_i != i || id > u32::MAX as u64 {
            return None;
        }
        SdWaitTarget::Id(id as u32)
    };

    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return Some((target, SDWAIT_DEFAULT_TIMEOUT_MS));
    }

    let (timeout_ms, next_i) = parse_u64_ascii(trimmed, i)?;
    if timeout_ms > u32::MAX as u64 {
        return None;
    }
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }
    Some((target, timeout_ms as u32))
}

fn parse_sdrwverify_command(line: &[u8]) -> Option<u32> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDRWVERIFY";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (lba, next_i) = parse_u64_ascii(trimmed, i)?;
    if lba > u32::MAX as u64 {
        return None;
    }
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }

    Some(lba as u32)
}

fn parse_sdfatls_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATLS";
    if !trimmed.starts_with(cmd) {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        let mut path = [0u8; SD_PATH_MAX];
        path[0] = b'/';
        return Some((path, 1));
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    let mut j = next_i;
    while j < trimmed.len() && trimmed[j].is_ascii_whitespace() {
        j += 1;
    }
    if j != trimmed.len() {
        return None;
    }
    Some((path, path_len))
}

fn parse_sdfatread_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATREAD";
    if !trimmed.starts_with(cmd) {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    let mut j = next_i;
    while j < trimmed.len() && trimmed[j].is_ascii_whitespace() {
        j += 1;
    }
    if j != trimmed.len() {
        return None;
    }
    Some((path, path_len))
}

fn parse_sdfatwrite_command(
    line: &[u8],
) -> Option<([u8; SD_PATH_MAX], u8, [u8; SD_WRITE_MAX], u16)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATWRITE";
    if !trimmed.starts_with(cmd) {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i > trimmed.len() {
        return None;
    }

    let payload = &trimmed[i..];
    if payload.len() > SD_WRITE_MAX {
        return None;
    }

    let mut data = [0u8; SD_WRITE_MAX];
    data[..payload.len()].copy_from_slice(payload);
    Some((path, path_len, data, payload.len() as u16))
}

fn parse_sdfatstat_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_command(line, b"SDFATSTAT")
}

fn parse_sdfatmkdir_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_command(line, b"SDFATMKDIR")
}

fn parse_sdfatrm_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_command(line, b"SDFATRM")
}

fn parse_sdfatren_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8, [u8; SD_PATH_MAX], u8)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATREN";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (src_path, src_path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (dst_path, dst_path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }

    Some((src_path, src_path_len, dst_path, dst_path_len))
}

fn parse_sdfatappend_command(
    line: &[u8],
) -> Option<([u8; SD_PATH_MAX], u8, [u8; SD_WRITE_MAX], u16)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATAPPEND";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i > trimmed.len() {
        return None;
    }

    let payload = &trimmed[i..];
    if payload.len() > SD_WRITE_MAX {
        return None;
    }

    let mut data = [0u8; SD_WRITE_MAX];
    data[..payload.len()].copy_from_slice(payload);
    Some((path, path_len, data, payload.len() as u16))
}

fn parse_sdfattrunc_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8, u32)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATTRUNC";
    if !trimmed.starts_with(cmd) {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (size, next_i) = parse_u64_ascii(trimmed, i)?;
    if size > u32::MAX as u64 {
        return None;
    }
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }

    Some((path, path_len, size as u32))
}

fn parse_single_path_command(line: &[u8], cmd: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    let trimmed = trim_ascii_whitespace(line);
    if !trimmed.starts_with(cmd) {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }
    Some((path, path_len))
}

fn parse_path_token(line: &[u8], start: usize) -> Option<([u8; SD_PATH_MAX], u8, usize)> {
    if start >= line.len() {
        return None;
    }
    let mut end = start;
    while end < line.len() && !line[end].is_ascii_whitespace() {
        end += 1;
    }
    if end == start {
        return None;
    }
    let token = &line[start..end];
    if token.len() > SD_PATH_MAX {
        return None;
    }
    let mut out = [0u8; SD_PATH_MAX];
    out[..token.len()].copy_from_slice(token);
    Some((out, token.len() as u8, end))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn path_from(buf: &[u8; SD_PATH_MAX], len: u8) -> &str {
        core::str::from_utf8(&buf[..len as usize]).unwrap()
    }

    #[test]
    fn parses_sdfatstat() {
        let cmd = parse_serial_command(b"SDFATSTAT /notes/TODO.txt");
        match cmd {
            Some(SerialCommand::FatStat { path, path_len }) => {
                assert_eq!(path_from(&path, path_len), "/notes/TODO.txt");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_sdfatmkdir() {
        let cmd = parse_serial_command(b"SDFATMKDIR /logs");
        match cmd {
            Some(SerialCommand::FatMkdir { path, path_len }) => {
                assert_eq!(path_from(&path, path_len), "/logs");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_sdfatrename() {
        let cmd = parse_serial_command(b"SDFATREN /old/name.txt /new/name.txt");
        match cmd {
            Some(SerialCommand::FatRename {
                src_path,
                src_path_len,
                dst_path,
                dst_path_len,
            }) => {
                assert_eq!(path_from(&src_path, src_path_len), "/old/name.txt");
                assert_eq!(path_from(&dst_path, dst_path_len), "/new/name.txt");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_sdfatappend() {
        let cmd = parse_serial_command(b"SDFATAPPEND /notes/log.txt hello");
        match cmd {
            Some(SerialCommand::FatAppend {
                path,
                path_len,
                data,
                data_len,
            }) => {
                assert_eq!(path_from(&path, path_len), "/notes/log.txt");
                assert_eq!(&data[..data_len as usize], b"hello");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_sdrwverify() {
        let cmd = parse_serial_command(b"SDRWVERIFY 2048");
        match cmd {
            Some(SerialCommand::RwVerify { lba }) => assert_eq!(lba, 2048),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_sdwait_defaults() {
        let cmd = parse_serial_command(b"SDWAIT");
        match cmd {
            Some(SerialCommand::SdWait { target, timeout_ms }) => {
                assert!(matches!(target, SdWaitTarget::Next));
                assert_eq!(timeout_ms, SDWAIT_DEFAULT_TIMEOUT_MS);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_sdwait_last_with_timeout() {
        let cmd = parse_serial_command(b"SDWAIT LAST 2500");
        match cmd {
            Some(SerialCommand::SdWait { target, timeout_ms }) => {
                assert!(matches!(target, SdWaitTarget::Last));
                assert_eq!(timeout_ms, 2500);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_sdwait_by_id() {
        let cmd = parse_serial_command(b"SDWAIT 42");
        match cmd {
            Some(SerialCommand::SdWait { target, timeout_ms }) => {
                match target {
                    SdWaitTarget::Id(id) => assert_eq!(id, 42),
                    _ => panic!("unexpected target"),
                }
                assert_eq!(timeout_ms, SDWAIT_DEFAULT_TIMEOUT_MS);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn rejects_sdwait_invalid_trailing_tokens() {
        let cmd = parse_serial_command(b"SDWAIT 42 100 extra");
        assert!(cmd.is_none());
    }

    #[test]
    fn rejects_oversized_sdfatwrite_payload() {
        let mut line = heapless::Vec::<u8, 512>::new();
        line.extend_from_slice(b"SDFATWRITE /notes/big.txt ")
            .expect("prefix");
        for _ in 0..(SD_WRITE_MAX + 1) {
            line.push(b'x').expect("payload");
        }
        let cmd = parse_serial_command(&line);
        assert!(cmd.is_none());
    }

    #[test]
    fn parses_sdfattrunc() {
        let cmd = parse_serial_command(b"SDFATTRUNC /notes/log.txt 1024");
        match cmd {
            Some(SerialCommand::FatTruncate {
                path,
                path_len,
                size,
            }) => {
                assert_eq!(path_from(&path, path_len), "/notes/log.txt");
                assert_eq!(size, 1024);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn rejects_bad_sdfatrename() {
        let cmd = parse_serial_command(b"SDFATREN /only_one_arg");
        assert!(cmd.is_none());
    }

    #[test]
    fn maps_timeset_to_event_and_responses() {
        let cmd = parse_serial_command(b"TIMESET 1762531200 -300").expect("command");
        let (app_event, sd_command, ok, busy) = serial_command_event_and_responses(cmd);
        assert!(sd_command.is_none());
        match app_event {
            Some(AppEvent::TimeSync(sync)) => {
                assert_eq!(sync.unix_epoch_utc_seconds, 1_762_531_200);
                assert_eq!(sync.tz_offset_minutes, -300);
            }
            _ => panic!("expected timesync event"),
        };
        assert_eq!(ok, b"TIMESET OK\r\n");
        assert_eq!(busy, b"TIMESET BUSY\r\n");
    }

    #[test]
    fn maps_sdfatstat_to_event_and_responses() {
        let cmd = parse_serial_command(b"SDFATSTAT /notes/TODO.txt").expect("command");
        let (app_event, sd_command, ok, busy) = serial_command_event_and_responses(cmd);
        assert!(app_event.is_none());
        match sd_command {
            Some(SdCommand::FatStat { path, path_len }) => {
                assert_eq!(path_from(&path, path_len), "/notes/TODO.txt");
            }
            _ => panic!("expected sdfat stat event"),
        };
        assert_eq!(ok, b"SDFATSTAT OK\r\n");
        assert_eq!(busy, b"SDFATSTAT BUSY\r\n");
    }

    #[test]
    fn maps_sdfatren_to_event_and_responses() {
        let cmd = parse_serial_command(b"SDFATREN /a.txt /b.txt").expect("command");
        let (app_event, sd_command, ok, busy) = serial_command_event_and_responses(cmd);
        assert!(app_event.is_none());
        match sd_command {
            Some(SdCommand::FatRename {
                src_path,
                src_path_len,
                dst_path,
                dst_path_len,
            }) => {
                assert_eq!(path_from(&src_path, src_path_len), "/a.txt");
                assert_eq!(path_from(&dst_path, dst_path_len), "/b.txt");
            }
            _ => panic!("expected sdfat rename event"),
        };
        assert_eq!(ok, b"SDFATREN OK\r\n");
        assert_eq!(busy, b"SDFATREN BUSY\r\n");
    }
}
