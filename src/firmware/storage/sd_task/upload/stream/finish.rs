use sdcard::fat;

use super::{
    div_or_zero, ensure_upload_ready, map_fat_error_to_upload_code, upload_result,
    write_metrics_delta, SdProbeDriver, SdUploadResult, SdUploadResultCode, SdUploadSession,
};

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
    log_commit_metrics(active, sd_probe, final_path_str);
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

fn log_commit_metrics(active: &SdUploadSession, sd_probe: &mut SdProbeDriver, final_path_str: &str) {
    let write_metrics_delta = write_metrics_delta(active.write_metrics_start, sd_probe.write_metrics_snapshot());
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
    let chunk_queue_wait_ms_avg = div_or_zero(
        active.chunk_timing.chunk_queue_wait_ms_total,
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
    let chunk_non_append_ms_avg = div_or_zero(
        active.chunk_timing.chunk_non_append_ms_total,
        active.chunk_timing.chunk_count,
    );
    let chunk_residual_ms_avg = div_or_zero(
        active.chunk_timing.chunk_residual_ms_total,
        active.chunk_timing.chunk_count,
    );
    esp_println::println!(
        "sd_upload: write_metrics path={} bytes={} cmd24_sectors={} cmd25_attempt_bursts={} cmd25_success_bursts={} cmd25_fallback_bursts={} cmd25_attempt_sectors={} cmd25_success_sectors={} cmd25_success_burst_ms_total={} cmd25_success_burst_ms_avg={} cmd25_ready_wait_count={} cmd25_ready_wait_ms_total={} cmd25_ready_wait_ms_avg={} cmd25_ready_wait_polls_total={} cmd25_ready_wait_polls_avg={} cmd25_ready_wait_over_1ms={} cmd25_ready_wait_over_4ms={} cmd25_ready_wait_over_8ms={} chunk_count={} chunk_queue_wait_ms_total={} chunk_queue_wait_ms_avg={} chunk_queue_wait_ms_max={} chunk_total_ms_total={} chunk_total_ms_avg={} chunk_total_ms_max={} chunk_total_over_200ms={} chunk_total_over_400ms={} chunk_ensure_ready_ms_total={} chunk_ensure_ready_ms_avg={} chunk_ensure_ready_ms_max={} chunk_payload_lock_ms_total={} chunk_payload_lock_ms_avg={} chunk_payload_lock_ms_max={} chunk_append_ms_total={} chunk_append_ms_avg={} chunk_append_ms_max={} chunk_append_over_200ms={} chunk_append_over_400ms={} chunk_append_capacity_ms_total={} chunk_append_capacity_ms_avg={} chunk_append_capacity_ms_max={} chunk_append_write_data_ms_total={} chunk_append_write_data_ms_avg={} chunk_append_write_data_ms_max={} chunk_non_append_ms_total={} chunk_non_append_ms_avg={} chunk_non_append_ms_max={} chunk_residual_ms_total={} chunk_residual_ms_avg={} chunk_residual_ms_max={} chunk_overhead_ms_total={} chunk_overhead_ms_avg={} chunk_overhead_ms_max={}",
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
        active.chunk_timing.chunk_queue_wait_ms_total,
        chunk_queue_wait_ms_avg,
        active.chunk_timing.chunk_queue_wait_ms_max,
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
        active.chunk_timing.chunk_non_append_ms_total,
        chunk_non_append_ms_avg,
        active.chunk_timing.chunk_non_append_ms_max,
        active.chunk_timing.chunk_residual_ms_total,
        chunk_residual_ms_avg,
        active.chunk_timing.chunk_residual_ms_max,
        active.chunk_timing.chunk_overhead_ms_total,
        chunk_overhead_ms_avg,
        active.chunk_timing.chunk_overhead_ms_max,
    );
}
