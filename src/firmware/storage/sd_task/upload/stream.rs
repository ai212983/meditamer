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

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        esp_println::println!(
            "sd_upload: chunk ensure_upload_ready failed code={:?} bytes_written={} data_len={}",
            code,
            active.bytes_written,
            data_len
        );
        return upload_result(false, code, active.bytes_written);
    }

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
    if let Err(err) = fat::append_session_write(
        sd_probe,
        &mut active.append_session,
        &chunk_data.as_mut_slice()[..data_len],
    )
    .await
    {
        esp_println::println!(
            "sd_upload: chunk append_session_write failed err={:?} bytes_written={} data_len={}",
            err,
            active.bytes_written,
            data_len
        );
        return upload_result(
            false,
            map_fat_error_to_upload_code(&err),
            active.bytes_written,
        );
    }
    active.bytes_written = next_bytes_written;
    active.last_activity_at = Instant::now();
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
    esp_println::println!(
        "sd_upload: write_metrics path={} bytes={} cmd24_sectors={} cmd25_attempt_bursts={} cmd25_success_bursts={} cmd25_fallback_bursts={} cmd25_attempt_sectors={} cmd25_success_sectors={}",
        final_path_str,
        active.bytes_written,
        write_metrics_delta.cmd24_sectors,
        write_metrics_delta.cmd25_attempt_bursts,
        write_metrics_delta.cmd25_success_bursts,
        write_metrics_delta.cmd25_fallback_bursts,
        write_metrics_delta.cmd25_attempt_sectors,
        write_metrics_delta.cmd25_success_sectors,
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
