use sdcard::fat::{FatEngine, FatResult};

use crate::firmware::storage::transfer_buffers;

use super::{
    elapsed_ms_u32, ensure_upload_ready, map_fat_result_to_upload_code, upload_result,
    SdProbeDriver, SdUploadChunkTimingSample, SdUploadResult, SdUploadResultCode, SdUploadSession,
    SD_UPLOAD_CHUNK_MAX, SD_UPLOAD_PATH_BUF_MAX,
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
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    let Some(active) = session.as_mut() else {
        return upload_result(false, SdUploadResultCode::SessionNotActive, 0);
    };
    let chunk_started_at = Instant::now();
    let data_len = (data_len as usize).min(SD_UPLOAD_CHUNK_MAX);
    if data_len == 0 {
        return upload_result(true, SdUploadResultCode::Ok, active.core.bytes_written());
    }

    if active
        .core
        .bytes_written()
        .checked_add(data_len as u32)
        .is_none()
    {
        return upload_result(
            false,
            SdUploadResultCode::SizeMismatch,
            active.core.bytes_written(),
        );
    }
    if data_len as u32 > active.core.remaining() {
        return upload_result(
            false,
            SdUploadResultCode::SizeMismatch,
            active.core.bytes_written(),
        );
    }

    let ensure_ready_started_at = Instant::now();
    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        let ensure_ready_ms = elapsed_ms_u32(ensure_ready_started_at);
        console::println!(
            "sd_upload: chunk ensure_upload_ready failed code={:?} bytes_written={} data_len={} ensure_ready_ms={}",
            code,
            active.core.bytes_written(),
            data_len,
            ensure_ready_ms,
        );
        return upload_result(false, code, active.core.bytes_written());
    }
    let ensure_ready_ms = elapsed_ms_u32(ensure_ready_started_at);

    let payload_lock_started_at = Instant::now();
    let mut chunk_data = match transfer_buffers::lock_upload_chunk_buffer().await {
        Ok(buffer) => buffer,
        Err(_) => {
            return upload_result(
                false,
                SdUploadResultCode::OperationFailed,
                active.core.bytes_written(),
            );
        }
    };
    let payload_lock_ms = elapsed_ms_u32(payload_lock_started_at);
    let append_started_at = Instant::now();
    let mut output = [];
    let result = super::super::super::engine_driver::run_fat_request(
        sdcard::upload::Session::<SD_UPLOAD_PATH_BUF_MAX>::chunk_request(data_len as u32),
        sd_probe,
        fat_engine,
        &chunk_data.as_mut_slice()[..data_len],
        &mut output,
    )
    .await;
    let append_total_ms = elapsed_ms_u32(append_started_at);
    if !matches!(result, FatResult::Done) {
        console::println!(
            "sd_upload: chunk engine failed result={:?} bytes_written={} data_len={} append_total_ms={}",
            result,
            active.core.bytes_written(),
            data_len,
            append_total_ms,
        );
        return upload_result(
            false,
            map_fat_result_to_upload_code(&result),
            active.core.bytes_written(),
        );
    }
    // Update the shared session only after the FAT append has succeeded, so a
    // failed transport cannot make a retry appear to have advanced.
    if active.core.accept_chunk(data_len as u32).is_err() {
        return upload_result(
            false,
            SdUploadResultCode::SizeMismatch,
            active.core.bytes_written(),
        );
    }
    let chunk_total_ms = elapsed_ms_u32(chunk_started_at);
    active.chunk_timing.record_chunk(SdUploadChunkTimingSample {
        queue_wait_ms,
        total_ms: chunk_total_ms,
        ensure_ready_ms,
        payload_lock_ms,
        append_total_ms,
        append_capacity_ms: 0,
        append_write_data_ms: append_total_ms,
    });
    upload_result(true, SdUploadResultCode::Ok, active.core.bytes_written())
}
