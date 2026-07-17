#![cfg_attr(feature = "asset-upload-http", allow(dead_code))]

use sdcard::fat::{FatEngine, FatEngineError, FatPayloadId, FatRequest, FatResult, SdFatError};

use super::super::super::types::{
    SdAssetReadRequest, SdAssetReadResponse, SdAssetReadResultCode, SdProbeDriver,
};
use super::super::transfer_buffers;
use super::upload::{ensure_upload_ready, SdUploadSession};

const SD_ASSET_ROOT: &str = "/assets";

pub(super) async fn process_asset_read_request(
    request: SdAssetReadRequest,
    upload_session: &Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdAssetReadResponse {
    if upload_session.is_some() {
        return asset_read_response(false, SdAssetReadResultCode::Busy, 0);
    }

    let path = match parse_asset_path(&request.path, request.path_len) {
        Ok(path) => path,
        Err(code) => return asset_read_response(false, code, 0),
    };

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        return asset_read_response(false, map_upload_ready_error(code), 0);
    }

    let mut data = match transfer_buffers::lock_asset_read_buffer().await {
        Ok(buffer) => buffer,
        Err(_) => return asset_read_response(false, SdAssetReadResultCode::OperationFailed, 0),
    };
    let mut fat_path = [0u8; sdcard::SD_PATH_MAX];
    fat_path[..path.len()].copy_from_slice(path.as_bytes());
    let result = super::engine_driver::run_fat_request(
        FatRequest::Read {
            path: fat_path,
            path_len: path.len() as u8,
            output: FatPayloadId::Primary,
            output_capacity: data.as_mut_slice().len() as u32,
        },
        sd_probe,
        fat_engine,
        &[],
        data.as_mut_slice(),
    )
    .await;
    match result {
        FatResult::Read { bytes } => {
            asset_read_response(true, SdAssetReadResultCode::Ok, bytes as u16)
        }
        result => asset_read_response(false, map_fat_result_to_asset_code(&result), 0),
    }
}

fn asset_read_response(
    ok: bool,
    code: SdAssetReadResultCode,
    data_len: u16,
) -> SdAssetReadResponse {
    SdAssetReadResponse { ok, code, data_len }
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

fn map_fat_result_to_asset_code(result: &FatResult) -> SdAssetReadResultCode {
    match result {
        FatResult::Error(FatEngineError::Fat(SdFatError::InvalidPath)) => {
            SdAssetReadResultCode::InvalidPath
        }
        FatResult::Error(FatEngineError::Fat(SdFatError::NotFound)) => {
            SdAssetReadResultCode::NotFound
        }
        FatResult::Error(FatEngineError::Fat(SdFatError::BufferTooSmall { .. })) => {
            SdAssetReadResultCode::SizeMismatch
        }
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
