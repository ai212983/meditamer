use sdcard::fat;

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

pub(super) fn map_fat_error_to_upload_code(error: &fat::SdFatError) -> SdUploadResultCode {
    match error {
        fat::SdFatError::InvalidPath => SdUploadResultCode::InvalidPath,
        fat::SdFatError::NotFound => SdUploadResultCode::NotFound,
        fat::SdFatError::NotEmpty => SdUploadResultCode::NotEmpty,
        fat::SdFatError::DirFull => SdUploadResultCode::DirectoryFull,
        _ => SdUploadResultCode::OperationFailed,
    }
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
    }
}
