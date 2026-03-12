use sdcard::fat;

use crate::firmware::storage::transfer_buffers;

use super::{
    elapsed_ms_u32, ensure_upload_ready, map_fat_error_to_upload_code, upload_result,
    SdProbeDriver, SdUploadResult, SdUploadResultCode, SdUploadSession, SD_UPLOAD_CHUNK_MAX,
};
use embassy_time::Instant;

#[inline(never)]
pub(super) async fn handle_chunk(
    data_len: u32,
    queue_wait_ms: u32,
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
        queue_wait_ms,
        chunk_total_ms,
        ensure_ready_ms,
        payload_lock_ms,
        append_diag.total_ms,
        append_diag.ensure_capacity_ms,
        append_diag.write_data_ms,
    );
    upload_result(true, SdUploadResultCode::Ok, active.bytes_written)
}
