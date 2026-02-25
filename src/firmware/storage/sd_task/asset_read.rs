#![cfg_attr(feature = "asset-upload-http", allow(dead_code))]

use sdcard::fat;

use super::super::super::types::{
    SdAssetReadRequest, SdAssetReadResponse, SdAssetReadResultCode, SdProbeDriver,
    SD_ASSET_READ_MAX,
};
use super::upload::{ensure_upload_ready, SdUploadSession};

const SD_ASSET_ROOT: &str = "/assets";

pub(super) async fn process_asset_read_request(
    request: SdAssetReadRequest,
    upload_session: &Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdAssetReadResponse {
    if upload_session.is_some() {
        return asset_read_response(
            false,
            SdAssetReadResultCode::Busy,
            [0; SD_ASSET_READ_MAX],
            0,
        );
    }

    let path = match parse_asset_path(&request.path, request.path_len) {
        Ok(path) => path,
        Err(code) => return asset_read_response(false, code, [0; SD_ASSET_READ_MAX], 0),
    };

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        return asset_read_response(
            false,
            map_upload_ready_error(code),
            [0; SD_ASSET_READ_MAX],
            0,
        );
    }

    let mut data = [0u8; SD_ASSET_READ_MAX];
    match fat::read_file(sd_probe, path, &mut data).await {
        Ok(data_len) => asset_read_response(true, SdAssetReadResultCode::Ok, data, data_len as u16),
        Err(err) => asset_read_response(false, map_fat_error_to_asset_code(&err), data, 0),
    }
}

fn asset_read_response(
    ok: bool,
    code: SdAssetReadResultCode,
    data: [u8; SD_ASSET_READ_MAX],
    data_len: u16,
) -> SdAssetReadResponse {
    SdAssetReadResponse {
        ok,
        code,
        data,
        data_len,
    }
}

fn map_upload_ready_error(
    code: super::super::super::types::SdUploadResultCode,
) -> SdAssetReadResultCode {
    match code {
        super::super::super::types::SdUploadResultCode::PowerOnFailed => {
            SdAssetReadResultCode::PowerOnFailed
        }
        super::super::super::types::SdUploadResultCode::InitFailed => {
            SdAssetReadResultCode::InitFailed
        }
        _ => SdAssetReadResultCode::OperationFailed,
    }
}

fn map_fat_error_to_asset_code(error: &fat::SdFatError) -> SdAssetReadResultCode {
    match error {
        fat::SdFatError::InvalidPath => SdAssetReadResultCode::InvalidPath,
        fat::SdFatError::NotFound => SdAssetReadResultCode::NotFound,
        fat::SdFatError::BufferTooSmall { .. } => SdAssetReadResultCode::SizeMismatch,
        _ => SdAssetReadResultCode::OperationFailed,
    }
}

pub(super) fn parse_asset_path(path: &[u8], path_len: u8) -> Result<&str, SdAssetReadResultCode> {
    let path_len = path_len as usize;
    if path_len == 0 || path_len > path.len() {
        return Err(SdAssetReadResultCode::InvalidPath);
    }
    let path_str =
        core::str::from_utf8(&path[..path_len]).map_err(|_| SdAssetReadResultCode::InvalidPath)?;
    if !path_str.starts_with('/') {
        return Err(SdAssetReadResultCode::InvalidPath);
    }

    if path_str != SD_ASSET_ROOT
        && (!path_str.starts_with(SD_ASSET_ROOT)
            || path_str.as_bytes().get(SD_ASSET_ROOT.len()) != Some(&b'/'))
    {
        return Err(SdAssetReadResultCode::InvalidPath);
    }

    for segment in path_str.split('/').skip(1) {
        if segment == "." || segment == ".." || segment.chars().any(|ch| ch.is_control()) {
            return Err(SdAssetReadResultCode::InvalidPath);
        }
    }

    Ok(path_str)
}
