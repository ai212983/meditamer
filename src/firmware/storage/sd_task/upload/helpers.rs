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

pub(super) fn copy_fat_path(
    path: &[u8],
) -> Result<([u8; sdcard::SD_PATH_MAX], u8), SdUploadResultCode> {
    if path.is_empty() || path.len() > sdcard::SD_PATH_MAX {
        return Err(SdUploadResultCode::InvalidPath);
    }
    let mut out = [0u8; sdcard::SD_PATH_MAX];
    out[..path.len()].copy_from_slice(path);
    Ok((out, path.len() as u8))
}

pub(super) fn parse_upload_path(path: &[u8], path_len: u8) -> Result<&str, SdUploadResultCode> {
    let path_len = path_len as usize;
    if path_len == 0 || path_len > path.len() {
        return Err(SdUploadResultCode::InvalidPath);
    }
    let path_str =
        core::str::from_utf8(&path[..path_len]).map_err(|_| SdUploadResultCode::InvalidPath)?;
    if !path_str.starts_with('/') {
        return Err(SdUploadResultCode::InvalidPath);
    }

    let root = SD_UPLOAD_ROOT;
    if path_str != root
        && (!path_str.starts_with(root) || path_str.as_bytes().get(root.len()) != Some(&b'/'))
    {
        return Err(SdUploadResultCode::InvalidPath);
    }

    for segment in path_str.split('/').skip(1) {
        if segment == "." || segment == ".." || segment.chars().any(|ch| ch.is_control()) {
            return Err(SdUploadResultCode::InvalidPath);
        }
    }

    Ok(path_str)
}

pub(super) fn upload_result(
    ok: bool,
    code: SdUploadResultCode,
    bytes_written: u32,
) -> SdUploadResult {
    SdUploadResult {
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
