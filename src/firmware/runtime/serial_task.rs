mod commands;
mod io;
mod labels;
mod parser;
mod queue;

use core::{fmt::Write, sync::atomic::Ordering};

use embassy_time::{with_timeout, Duration};

use super::super::{
    config::{
        LAST_MARBLE_REDRAW_MS, MAX_MARBLE_REDRAW_MS, MODE_APPLY_ACK_TIMEOUT_MS, SD_RESULTS,
        TAP_TRACE_ENABLED, TAP_TRACE_SAMPLES, TIMESET_CMD_BUF_LEN,
    },
    telemetry,
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
    types::{AppEvent, SdCommand, SdRequest, SdResult, SerialUart},
};

use commands::{
    runtime_services_update_for_command, serial_command_event_and_responses, SerialCommand,
};
#[cfg(feature = "asset-upload-http")]
use io::run_wifiset_command;
use io::{
    cache_sd_result, drain_runtime_services_apply_acks, run_allocator_alloc_probe,
    run_sdwait_command, wait_runtime_services_apply_ack, write_allocator_status_line,
    write_mode_status_line, write_sd_request_queued, write_sd_result, write_tap_trace_sample,
    SD_RESULT_CACHE_CAP,
};
use parser::parse_serial_command;
use queue::{enqueue_app_event_with_retry, enqueue_sd_request_with_retry};

#[embassy_executor::task]
pub(crate) async fn time_sync_task(mut uart: SerialUart) {
    let mut line_buf = [0u8; TIMESET_CMD_BUF_LEN];
    let mut line_len = 0usize;
    let mut rx = [0u8; 1];
    let mut touch_wizard_log = TouchWizardSessionLog::new();
    let mut next_sd_request_id = 1u32;
    let mut next_mode_request_id = 1u16;
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
                        SerialCommand::Ping => {
                            let _ = uart_write_all(&mut uart, b"PONG\r\n").await;
                        }
                        SerialCommand::Metrics => {
                            let last_ms = LAST_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
                            let max_ms = MAX_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
                            let snapshot = telemetry::snapshot();

                            let mut line = heapless::String::<160>::new();
                            let _ = write!(
                                &mut line,
                                "METRICS MARBLE_REDRAW_MS={} MAX_MS={}\r\n",
                                last_ms, max_ms
                            );
                            let _ = uart_write_all(&mut uart, line.as_bytes()).await;

                            let mut wifi_line = heapless::String::<256>::new();
                            let _ = write!(
                                &mut wifi_line,
                                "METRICS WIFI attempt={} success={} failure={} no_ap={} scan_runs={} scan_empty={} scan_hits={}\r\n",
                                snapshot.wifi_connect_attempts,
                                snapshot.wifi_connect_successes,
                                snapshot.wifi_connect_failures,
                                snapshot.wifi_reason_no_ap_found,
                                snapshot.wifi_scan_runs,
                                snapshot.wifi_scan_empty,
                                snapshot.wifi_scan_target_hits,
                            );
                            let _ = uart_write_all(&mut uart, wifi_line.as_bytes()).await;

                            let mut upload_line = heapless::String::<320>::new();
                            let _ = write!(
                                &mut upload_line,
                                "METRICS UPLOAD accept_ok={} accept_err={} request_err={} req_hdr_to={} req_read_body={} req_sd_busy={} sd_errors={} sd_busy={} sd_timeouts={} sd_power_on_fail={} sd_init_fail={} sess_timeout_abort={} sess_mode_off_abort={}\r\n",
                                snapshot.upload_http_accepts,
                                snapshot.upload_http_accept_errors,
                                snapshot.upload_http_request_errors,
                                snapshot.upload_http_header_timeouts,
                                snapshot.upload_http_read_body_errors,
                                snapshot.upload_http_sd_busy_errors,
                                snapshot.sd_upload_errors,
                                snapshot.sd_upload_busy,
                                snapshot.sd_upload_timeouts,
                                snapshot.sd_upload_power_on_failed,
                                snapshot.sd_upload_init_failed,
                                snapshot.sd_upload_session_timeout_aborts,
                                snapshot.sd_upload_session_mode_off_aborts,
                            );
                            let _ = uart_write_all(&mut uart, upload_line.as_bytes()).await;

                            let mut upload_phase_line = heapless::String::<320>::new();
                            let _ = write!(
                                &mut upload_phase_line,
                                "METRICS UPLOAD_PHASE req={} bytes={} body_ms={} body_max={} sd_ms={} sd_max={} req_ms={} req_max={}\r\n",
                                snapshot.upload_http_upload_requests,
                                snapshot.upload_http_upload_bytes,
                                snapshot.upload_http_upload_body_read_ms_total,
                                snapshot.upload_http_upload_body_read_ms_max,
                                snapshot.upload_http_upload_sd_wait_ms_total,
                                snapshot.upload_http_upload_sd_wait_ms_max,
                                snapshot.upload_http_upload_request_ms_total,
                                snapshot.upload_http_upload_request_ms_max,
                            );
                            let _ = uart_write_all(&mut uart, upload_phase_line.as_bytes()).await;

                            let mut upload_rtt_line = heapless::String::<512>::new();
                            let _ = write!(
                                &mut upload_rtt_line,
                                "METRICS UPLOAD_RTT begin_n={} begin_ms={} begin_max={} chunk_n={} chunk_ms={} chunk_max={} commit_n={} commit_ms={} commit_max={} abort_n={} abort_ms={} abort_max={} mkdir_n={} mkdir_ms={} mkdir_max={} rm_n={} rm_ms={} rm_max={}\r\n",
                                snapshot.sd_upload_rtt_begin_count,
                                snapshot.sd_upload_rtt_begin_ms_total,
                                snapshot.sd_upload_rtt_begin_ms_max,
                                snapshot.sd_upload_rtt_chunk_count,
                                snapshot.sd_upload_rtt_chunk_ms_total,
                                snapshot.sd_upload_rtt_chunk_ms_max,
                                snapshot.sd_upload_rtt_commit_count,
                                snapshot.sd_upload_rtt_commit_ms_total,
                                snapshot.sd_upload_rtt_commit_ms_max,
                                snapshot.sd_upload_rtt_abort_count,
                                snapshot.sd_upload_rtt_abort_ms_total,
                                snapshot.sd_upload_rtt_abort_ms_max,
                                snapshot.sd_upload_rtt_mkdir_count,
                                snapshot.sd_upload_rtt_mkdir_ms_total,
                                snapshot.sd_upload_rtt_mkdir_ms_max,
                                snapshot.sd_upload_rtt_remove_count,
                                snapshot.sd_upload_rtt_remove_ms_total,
                                snapshot.sd_upload_rtt_remove_ms_max,
                            );
                            let _ = uart_write_all(&mut uart, upload_rtt_line.as_bytes()).await;

                            let ip = snapshot.upload_http_ipv4.unwrap_or([0, 0, 0, 0]);
                            let mut net_line = heapless::String::<160>::new();
                            let _ = write!(
                                &mut net_line,
                                "METRICS NET wifi_connected={} http_listening={} ip={}.{}.{}.{}\r\n",
                                if snapshot.wifi_link_connected { 1 } else { 0 },
                                if snapshot.upload_http_listening { 1 } else { 0 },
                                ip[0],
                                ip[1],
                                ip[2],
                                ip[3],
                            );
                            let _ = uart_write_all(&mut uart, net_line.as_bytes()).await;
                        }
                        SerialCommand::AllocatorStatus => {
                            write_allocator_status_line(&mut uart).await;
                        }
                        SerialCommand::ModeStatus => {
                            write_mode_status_line(&mut uart).await;
                        }
                        SerialCommand::ModeSet { .. } | SerialCommand::RunMode { .. } => {
                            let update = runtime_services_update_for_command(cmd)
                                .expect("mode commands must map to runtime services updates");
                            let (ok_response, busy_response, timeout_response) = match cmd {
                                SerialCommand::ModeSet { .. } => (
                                    b"MODE OK\r\n".as_slice(),
                                    b"MODE BUSY\r\n".as_slice(),
                                    b"MODE ERR reason=timeout\r\n".as_slice(),
                                ),
                                SerialCommand::RunMode { .. } => (
                                    b"RUNMODE OK\r\n".as_slice(),
                                    b"RUNMODE BUSY\r\n".as_slice(),
                                    b"RUNMODE ERR reason=timeout\r\n".as_slice(),
                                ),
                                _ => unreachable!("non-mode command routed to mode handler"),
                            };
                            drain_runtime_services_apply_acks();
                            let request_id = next_mode_request_id;
                            next_mode_request_id = next_mode_request_id.wrapping_add(1);
                            let queued =
                                enqueue_app_event_with_retry(AppEvent::UpdateRuntimeServices {
                                    update,
                                    ack_request_id: Some(request_id),
                                })
                                .await;
                            if !queued {
                                let _ = uart.write_async(busy_response).await;
                                continue;
                            }
                            if wait_runtime_services_apply_ack(
                                request_id,
                                MODE_APPLY_ACK_TIMEOUT_MS,
                            )
                            .await
                            .is_some()
                            {
                                let _ = uart.write_async(ok_response).await;
                            } else {
                                let _ = uart.write_async(timeout_response).await;
                            }
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
mod tests;
