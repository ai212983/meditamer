use super::super::super::super::types::{
    SdProbeDriver, SdUploadResult, SdUploadResultCode, SD_PATH_MAX, SD_UPLOAD_CHUNK_MAX,
};
use super::super::SD_UPLOAD_PATH_BUF_MAX;
use super::helpers::{ensure_upload_ready, map_fat_error_to_upload_code, upload_result};
use super::metrics::write_metrics_delta;
use super::path_ops::{build_temp_upload_path, parse_upload_path};
use super::types::SdUploadSession;
use crate::firmware::telemetry;
use crate::firmware::storage::transfer_buffers;
use embassy_time::Instant;
use sdcard::fat;

#[inline(never)]
pub(super) async fn handle_begin(
    path: [u8; SD_PATH_MAX],
    path_len: u8,
    expected_size: u32,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    telemetry::log_stack_headroom("sd_upload_begin_entry");
    if session.is_some() {
        return upload_result(false, SdUploadResultCode::Busy, 0);
    }

    let final_path = match parse_upload_path(&path, path_len) {
        Ok(path) => path,
        Err(code) => return upload_result(false, code, 0),
    };
    esp_println::println!(
        "sd_upload: begin path={} expected_size={}",
        final_path,
        expected_size
    );
    let final_path_bytes = final_path.as_bytes();
    if final_path_bytes.len() > SD_UPLOAD_PATH_BUF_MAX {
        esp_println::println!(
            "sd_upload: begin final_path_too_long path_len={} max_len={}",
            final_path_bytes.len(),
            SD_UPLOAD_PATH_BUF_MAX
        );
        return upload_result(false, SdUploadResultCode::InvalidPath, 0);
    }

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        esp_println::println!(
            "sd_upload: begin ensure_upload_ready failed code={:?}",
            code
        );
        return upload_result(false, code, 0);
    }
    telemetry::log_stack_headroom("sd_upload_begin_ready");

    let (temp_path, temp_len) = match build_temp_upload_path(final_path_bytes) {
        Ok(path) => path,
        Err(code) => return upload_result(false, code, 0),
    };
    let temp_path_str = match core::str::from_utf8(&temp_path[..temp_len]) {
        Ok(path) => path,
        Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
    };

    telemetry::log_stack_headroom("sd_upload_begin_fat_before");
    let append_session =
        match fat::begin_append_session_create_or_open(sd_probe, temp_path_str).await {
            Ok(session) => session,
            Err(err) => {
                esp_println::println!(
                    "sd_upload: begin append_session_create_or_open failed temp_path={} err={:?}",
                    temp_path_str,
                    err
                );
                return upload_result(false, map_fat_error_to_upload_code(&err), 0);
            }
        };
    telemetry::log_stack_headroom("sd_upload_begin_fat_after");
    let mut final_path_buf = [0u8; SD_UPLOAD_PATH_BUF_MAX];
    final_path_buf[..final_path_bytes.len()].copy_from_slice(final_path_bytes);
    *session = Some(SdUploadSession {
        final_path: final_path_buf,
        final_path_len: final_path_bytes.len() as u8,
        temp_path,
        temp_path_len: temp_len as u8,
        append_session,
        expected_size,
        bytes_written: 0,
        last_activity_at: Instant::now(),
        write_metrics_start: sd_probe.write_metrics_snapshot(),
        chunk_timing: super::types::SdUploadChunkTimingMetrics::default(),
    });
    upload_result(true, SdUploadResultCode::Ok, 0)
}

#[inline(never)]
pub(super) async fn handle_chunk(
    data_len: u32,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    let Some(active) = session.as_mut() else {
        return upload_result(false, SdUploadResultCode::SessionNotActive, 0);
    };
    let chunk_started_at = Instant::now();
    let data_len = (data_len as usize).min(SD_UPLOAD_CHUNK_MAX);
    if data_len == 0 {
        return upload_result(true, SdUploadResultCode::Ok, active.bytes_written);
    }

    let Some(next_bytes_written) = active.bytes_written.checked_add(data_len as u32) else {
        return upload_result(
            false,
            SdUploadResultCode::SizeMismatch,
            active.bytes_written,
        );
    };
    if next_bytes_written > active.expected_size {
        return upload_result(
            false,
            SdUploadResultCode::SizeMismatch,
            active.bytes_written,
        );
    }

    let ensure_ready_started_at = Instant::now();
    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        let ensure_ready_ms = elapsed_ms_u32(ensure_ready_started_at);
        esp_println::println!(
            "sd_upload: chunk ensure_upload_ready failed code={:?} bytes_written={} data_len={} ensure_ready_ms={}",
            code,
            active.bytes_written,
            data_len,
            ensure_ready_ms,
        );
        return upload_result(false, code, active.bytes_written);
    }
    let ensure_ready_ms = elapsed_ms_u32(ensure_ready_started_at);

    let payload_lock_started_at = Instant::now();
    let mut chunk_data = match transfer_buffers::lock_upload_chunk_buffer().await {
        Ok(buffer) => buffer,
        Err(_) => {
            return upload_result(
                false,
                SdUploadResultCode::OperationFailed,
                active.bytes_written,
            );
        }
    };
    let payload_lock_ms = elapsed_ms_u32(payload_lock_started_at);
    let mut append_diag = fat::FatAppendWriteDiag::default();
    if let Err(err) = fat::append_session_write_with_diag(
        sd_probe,
        &mut active.append_session,
        &chunk_data.as_mut_slice()[..data_len],
        &mut append_diag,
    )
    .await
    {
        esp_println::println!(
            "sd_upload: chunk append_session_write failed err={:?} bytes_written={} data_len={} append_total_ms={} append_capacity_ms={} append_write_data_ms={}",
            err,
            active.bytes_written,
            data_len,
            append_diag.total_ms,
            append_diag.ensure_capacity_ms,
            append_diag.write_data_ms,
        );
        return upload_result(
            false,
            map_fat_error_to_upload_code(&err),
            active.bytes_written,
        );
    }
    active.bytes_written = next_bytes_written;
    active.last_activity_at = Instant::now();
    let chunk_total_ms = elapsed_ms_u32(chunk_started_at);
    active.chunk_timing.record_chunk(
        chunk_total_ms,
        ensure_ready_ms,
        payload_lock_ms,
        append_diag.total_ms,
        append_diag.ensure_capacity_ms,
        append_diag.write_data_ms,
    );
    upload_result(true, SdUploadResultCode::Ok, active.bytes_written)
}

#[inline(never)]
pub(super) async fn handle_commit(
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    let Some(active) = session.as_mut() else {
        return upload_result(false, SdUploadResultCode::SessionNotActive, 0);
    };
    if active.bytes_written != active.expected_size {
        return upload_result(
            false,
            SdUploadResultCode::SizeMismatch,
            active.bytes_written,
        );
    }

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        esp_println::println!(
            "sd_upload: commit ensure_upload_ready failed code={:?} bytes_written={} expected_size={}",
            code,
            active.bytes_written,
            active.expected_size
        );
        return upload_result(false, code, active.bytes_written);
    }

    let temp_path_str =
        match core::str::from_utf8(&active.temp_path[..active.temp_path_len as usize]) {
            Ok(path) => path,
            Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
        };
    let final_path_str =
        match core::str::from_utf8(&active.final_path[..active.final_path_len as usize]) {
            Ok(path) => path,
            Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
        };
    if let Err(err) = fat::append_session_flush(sd_probe, &active.append_session).await {
        esp_println::println!(
            "sd_upload: commit append_session_flush failed temp_path={} err={:?}",
            temp_path_str,
            err
        );
        return upload_result(
            false,
            map_fat_error_to_upload_code(&err),
            active.bytes_written,
        );
    }

    if let Err(err) = fat::rename_replace(sd_probe, temp_path_str, final_path_str).await {
        esp_println::println!(
            "sd_upload: commit rename_replace failed temp_path={} final_path={} err={:?}",
            temp_path_str,
            final_path_str,
            err
        );
        return upload_result(
            false,
            map_fat_error_to_upload_code(&err),
            active.bytes_written,
        );
    }
    let write_metrics_delta = write_metrics_delta(
        active.write_metrics_start,
        sd_probe.write_metrics_snapshot(),
    );
    let cmd25_success_burst_ms_avg = if write_metrics_delta.cmd25_success_bursts == 0 {
        0
    } else {
        write_metrics_delta.cmd25_success_burst_ms_total / write_metrics_delta.cmd25_success_bursts
    };
    let cmd25_ready_wait_ms_avg = if write_metrics_delta.cmd25_ready_wait_count == 0 {
        0
    } else {
        write_metrics_delta.cmd25_ready_wait_ms_total / write_metrics_delta.cmd25_ready_wait_count
    };
    let cmd25_ready_wait_polls_avg = if write_metrics_delta.cmd25_ready_wait_count == 0 {
        0
    } else {
        write_metrics_delta.cmd25_ready_wait_polls_total / write_metrics_delta.cmd25_ready_wait_count
    };
    let chunk_total_ms_avg = div_or_zero(
        active.chunk_timing.chunk_total_ms_total,
        active.chunk_timing.chunk_count,
    );
    let chunk_ensure_ready_ms_avg = div_or_zero(
        active.chunk_timing.ensure_ready_ms_total,
        active.chunk_timing.chunk_count,
    );
    let chunk_payload_lock_ms_avg = div_or_zero(
        active.chunk_timing.payload_lock_ms_total,
        active.chunk_timing.chunk_count,
    );
    let chunk_append_ms_avg = div_or_zero(
        active.chunk_timing.append_total_ms_total,
        active.chunk_timing.chunk_count,
    );
    let chunk_append_capacity_ms_avg = div_or_zero(
        active.chunk_timing.append_capacity_ms_total,
        active.chunk_timing.chunk_count,
    );
    let chunk_append_write_data_ms_avg = div_or_zero(
        active.chunk_timing.append_write_data_ms_total,
        active.chunk_timing.chunk_count,
    );
    let chunk_overhead_ms_avg = div_or_zero(
        active.chunk_timing.chunk_overhead_ms_total,
        active.chunk_timing.chunk_count,
    );
    esp_println::println!(
        "sd_upload: write_metrics path={} bytes={} cmd24_sectors={} cmd25_attempt_bursts={} cmd25_success_bursts={} cmd25_fallback_bursts={} cmd25_attempt_sectors={} cmd25_success_sectors={} cmd25_success_burst_ms_total={} cmd25_success_burst_ms_avg={} cmd25_ready_wait_count={} cmd25_ready_wait_ms_total={} cmd25_ready_wait_ms_avg={} cmd25_ready_wait_polls_total={} cmd25_ready_wait_polls_avg={} cmd25_ready_wait_over_1ms={} cmd25_ready_wait_over_4ms={} cmd25_ready_wait_over_8ms={} chunk_count={} chunk_total_ms_total={} chunk_total_ms_avg={} chunk_total_ms_max={} chunk_total_over_200ms={} chunk_total_over_400ms={} chunk_ensure_ready_ms_total={} chunk_ensure_ready_ms_avg={} chunk_ensure_ready_ms_max={} chunk_payload_lock_ms_total={} chunk_payload_lock_ms_avg={} chunk_payload_lock_ms_max={} chunk_append_ms_total={} chunk_append_ms_avg={} chunk_append_ms_max={} chunk_append_over_200ms={} chunk_append_over_400ms={} chunk_append_capacity_ms_total={} chunk_append_capacity_ms_avg={} chunk_append_capacity_ms_max={} chunk_append_write_data_ms_total={} chunk_append_write_data_ms_avg={} chunk_append_write_data_ms_max={} chunk_overhead_ms_total={} chunk_overhead_ms_avg={} chunk_overhead_ms_max={}",
        final_path_str,
        active.bytes_written,
        write_metrics_delta.cmd24_sectors,
        write_metrics_delta.cmd25_attempt_bursts,
        write_metrics_delta.cmd25_success_bursts,
        write_metrics_delta.cmd25_fallback_bursts,
        write_metrics_delta.cmd25_attempt_sectors,
        write_metrics_delta.cmd25_success_sectors,
        write_metrics_delta.cmd25_success_burst_ms_total,
        cmd25_success_burst_ms_avg,
        write_metrics_delta.cmd25_ready_wait_count,
        write_metrics_delta.cmd25_ready_wait_ms_total,
        cmd25_ready_wait_ms_avg,
        write_metrics_delta.cmd25_ready_wait_polls_total,
        cmd25_ready_wait_polls_avg,
        write_metrics_delta.cmd25_ready_wait_over_1ms,
        write_metrics_delta.cmd25_ready_wait_over_4ms,
        write_metrics_delta.cmd25_ready_wait_over_8ms,
        active.chunk_timing.chunk_count,
        active.chunk_timing.chunk_total_ms_total,
        chunk_total_ms_avg,
        active.chunk_timing.chunk_total_ms_max,
        active.chunk_timing.chunk_total_over_200ms,
        active.chunk_timing.chunk_total_over_400ms,
        active.chunk_timing.ensure_ready_ms_total,
        chunk_ensure_ready_ms_avg,
        active.chunk_timing.ensure_ready_ms_max,
        active.chunk_timing.payload_lock_ms_total,
        chunk_payload_lock_ms_avg,
        active.chunk_timing.payload_lock_ms_max,
        active.chunk_timing.append_total_ms_total,
        chunk_append_ms_avg,
        active.chunk_timing.append_total_ms_max,
        active.chunk_timing.append_total_over_200ms,
        active.chunk_timing.append_total_over_400ms,
        active.chunk_timing.append_capacity_ms_total,
        chunk_append_capacity_ms_avg,
        active.chunk_timing.append_capacity_ms_max,
        active.chunk_timing.append_write_data_ms_total,
        chunk_append_write_data_ms_avg,
        active.chunk_timing.append_write_data_ms_max,
        active.chunk_timing.chunk_overhead_ms_total,
        chunk_overhead_ms_avg,
        active.chunk_timing.chunk_overhead_ms_max,
    );
    let bytes_written = active.bytes_written;
    *session = None;
    upload_result(true, SdUploadResultCode::Ok, bytes_written)
}

#[inline(never)]
pub(super) async fn handle_abort(
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    let Some(active) = session.take() else {
        return upload_result(true, SdUploadResultCode::Ok, 0);
    };

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        esp_println::println!(
            "sd_upload: abort ensure_upload_ready failed code={:?} bytes_written={}",
            code,
            active.bytes_written
        );
        return upload_result(false, code, active.bytes_written);
    }

    let temp_path_str =
        match core::str::from_utf8(&active.temp_path[..active.temp_path_len as usize]) {
            Ok(path) => path,
            Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
        };
    match fat::remove(sd_probe, temp_path_str).await {
        Ok(()) | Err(fat::SdFatError::NotFound) => {
            upload_result(true, SdUploadResultCode::Ok, active.bytes_written)
        }
        Err(err) => {
            esp_println::println!(
                "sd_upload: abort remove failed temp_path={} err={:?}",
                temp_path_str,
                err
            );
            upload_result(
                false,
                map_fat_error_to_upload_code(&err),
                active.bytes_written,
            )
        }
    }
}

fn div_or_zero(total: u32, count: u32) -> u32 {
    if count == 0 {
        0
    } else {
        total / count
    }
}

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}
