use sdcard::fat::{FatEngineError, FatResult, SdFatError};

use super::super::super::super::types::{
    SdPowerRequest, SdProbeDriver, SdUploadResult, SdUploadResultCode,
};
use super::super::{request_sd_power, SD_UPLOAD_ROOT};

pub(super) async fn ensure_upload_ready(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> Result<(), SdUploadResultCode> {
    if *upload_mounted && !sd_probe.is_initialized() {
        *upload_mounted = false;
    }
    if !*powered {
        if !request_sd_power(SdPowerRequest::On).await {
            return Err(SdUploadResultCode::PowerOnFailed);
        }
        *powered = true;
        *upload_mounted = false;
    }

    if !*upload_mounted {
        if !sd_probe.is_initialized() {
            return Err(SdUploadResultCode::InitFailed);
        }
        *upload_mounted = true;
    }

    Ok(())
}

pub(super) fn map_fat_error_to_upload_code(error: &SdFatError) -> SdUploadResultCode {
    match error {
        SdFatError::InvalidPath => SdUploadResultCode::InvalidPath,
        SdFatError::NotFound => SdUploadResultCode::NotFound,
        SdFatError::NotEmpty => SdUploadResultCode::NotEmpty,
        SdFatError::DirFull => SdUploadResultCode::DirectoryFull,
        _ => SdUploadResultCode::OperationFailed,
    }
}

pub(super) fn map_fat_result_to_upload_code(result: &FatResult) -> SdUploadResultCode {
    match result {
        FatResult::Error(FatEngineError::Fat(error)) => map_fat_error_to_upload_code(error),
        _ => SdUploadResultCode::OperationFailed,
    }
}

pub(super) fn parse_upload_path(path: &[u8], path_len: u8) -> Result<&str, SdUploadResultCode> {
    sdcard::upload::validate_path(path, path_len as usize, SD_UPLOAD_ROOT)
        .map_err(|_| SdUploadResultCode::InvalidPath)
}

pub(super) fn upload_result(
    ok: bool,
    code: SdUploadResultCode,
    bytes_written: u32,
) -> SdUploadResult {
    SdUploadResult {
        request_id: 0,
        ok,
        code,
        bytes_written,
        chunk_queue_wait_ms: 0,
        chunk_handler_ms: 0,
        chunk_post_handler_ms: 0,
        chunk_published_at_ms: 0,
        chunk_handler_done_at_ms: 0,
    }
}
