mod commands;
mod io;
mod labels;
mod parser;
mod queue;

use core::{fmt::Write, sync::atomic::Ordering};

use embassy_time::{with_timeout, Duration};

#[cfg(all(test, feature = "asset-upload-http"))]
use super::super::types::RuntimeMode;
#[cfg(test)]
use super::super::types::{AppEvent, SD_PATH_MAX, SD_WRITE_MAX};
use super::super::{
    config::{
        LAST_MARBLE_REDRAW_MS, MAX_MARBLE_REDRAW_MS, SD_RESULTS, TAP_TRACE_ENABLED,
        TAP_TRACE_SAMPLES, TIMESET_CMD_BUF_LEN,
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
    types::{SdCommand, SdRequest, SdResult, SerialUart},
};

#[cfg(test)]
use commands::SdWaitTarget;
use commands::{serial_command_event_and_responses, SerialCommand};
#[cfg(feature = "asset-upload-http")]
use io::run_wifiset_command;
use io::{
    cache_sd_result, run_allocator_alloc_probe, run_sdwait_command, write_allocator_status_line,
    write_sd_request_queued, write_sd_result, write_tap_trace_sample, SD_RESULT_CACHE_CAP,
};
use parser::parse_serial_command;
#[cfg(test)]
use parser::SDWAIT_DEFAULT_TIMEOUT_MS;
use queue::{enqueue_app_event_with_retry, enqueue_sd_request_with_retry};

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
                        SerialCommand::AllocatorStatus => {
                            write_allocator_status_line(&mut uart).await;
                        }
                        SerialCommand::AllocatorAllocProbe { bytes } => {
                            run_allocator_alloc_probe(&mut uart, bytes as usize).await;
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
                        #[cfg(feature = "asset-upload-http")]
                        SerialCommand::WifiSet { credentials } => {
                            run_wifiset_command(&mut uart, credentials).await;
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
    fn parses_psram_allocator_status_command() {
        let cmd = parse_serial_command(b"PSRAM");
        assert!(matches!(cmd, Some(SerialCommand::AllocatorStatus)));
    }

    #[test]
    fn parses_heap_allocator_status_alias() {
        let cmd = parse_serial_command(b"HEAP");
        assert!(matches!(cmd, Some(SerialCommand::AllocatorStatus)));
    }

    #[test]
    fn parses_psram_alloc_probe_command() {
        let cmd = parse_serial_command(b"PSRAMALLOC 4096");
        match cmd {
            Some(SerialCommand::AllocatorAllocProbe { bytes }) => assert_eq!(bytes, 4096),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn rejects_psram_alloc_probe_without_size() {
        let cmd = parse_serial_command(b"PSRAMALLOC");
        assert!(cmd.is_none());
    }

    #[cfg(feature = "asset-upload-http")]
    #[test]
    fn parses_runmode_upload() {
        let cmd = parse_serial_command(b"RUNMODE UPLOAD");
        match cmd {
            Some(SerialCommand::RunMode { mode }) => {
                assert!(matches!(mode, RuntimeMode::Upload));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[cfg(feature = "asset-upload-http")]
    #[test]
    fn parses_runmode_normal_case_insensitive() {
        let cmd = parse_serial_command(b"runmode normal");
        match cmd {
            Some(SerialCommand::RunMode { mode }) => {
                assert!(matches!(mode, RuntimeMode::Normal));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[cfg(feature = "asset-upload-http")]
    #[test]
    fn parses_wifiset_with_password() {
        let cmd = parse_serial_command(b"WIFISET MyNet pass1234");
        match cmd {
            Some(SerialCommand::WifiSet { credentials }) => {
                assert_eq!(&credentials.ssid[..credentials.ssid_len as usize], b"MyNet");
                assert_eq!(
                    &credentials.password[..credentials.password_len as usize],
                    b"pass1234"
                );
            }
            _ => panic!("unexpected command"),
        }
    }

    #[cfg(feature = "asset-upload-http")]
    #[test]
    fn parses_wifiset_open_network() {
        let cmd = parse_serial_command(b"WIFISET CafeWiFi");
        match cmd {
            Some(SerialCommand::WifiSet { credentials }) => {
                assert_eq!(
                    &credentials.ssid[..credentials.ssid_len as usize],
                    b"CafeWiFi"
                );
                assert_eq!(credentials.password_len, 0);
            }
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
