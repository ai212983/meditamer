use sdcard::fat::{FatEngine, FatRequest, FatResult};

use crate::firmware::observability;

use super::super::super::super::types::{
    SdProbeDriver, SdUploadResult, SdUploadResultCode, SD_PATH_MAX,
};
use super::super::{SD_UPLOAD_PATH_BUF_MAX, SD_UPLOAD_ROOT, SD_UPLOAD_TMP_BASENAME};
use super::helpers::{ensure_upload_ready, map_fat_result_to_upload_code, upload_result};
use super::types::SdUploadSession;

#[inline(never)]
pub(super) async fn handle_mkdir(
    path: [u8; SD_PATH_MAX],
    path_len: u8,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    if observability::log_filter_enabled(observability::LOG_DOMAIN_SD) {
        console::println!("sd_upload: mkdir enter");
    }
    if session.is_some() {
        console::println!("sd_upload: mkdir busy(active session)");
        return upload_result(false, SdUploadResultCode::Busy, 0);
    }

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        console::println!(
            "sd_upload: mkdir ensure_upload_ready failed code={:?}",
            code
        );
        return upload_result(false, code, 0);
    }

    let path_str = match parse_upload_path(&path, path_len) {
        Ok(path) => path,
        Err(code) => {
            console::println!("sd_upload: mkdir invalid path code={:?}", code);
            return upload_result(false, code, 0);
        }
    };
    if observability::log_filter_enabled(observability::LOG_DOMAIN_SD) {
        console::println!("sd_upload: mkdir path={}", path_str);
    }

    let mut output = [];
    let result = super::super::engine_driver::run_fat_request(
        FatRequest::Mkdir { path, path_len },
        sd_probe,
        fat_engine,
        &[],
        &mut output,
    )
    .await;
    match sdcard::upload::normalize_path_result(sdcard::upload::PathOperation::Mkdir, result) {
        FatResult::Done => {
            if observability::log_filter_enabled(observability::LOG_DOMAIN_SD) {
                console::println!("sd_upload: mkdir ok/already_exists");
            }
            upload_result(true, SdUploadResultCode::Ok, 0)
        }
        result => {
            console::println!("sd_upload: mkdir engine error={:?}", result);
            upload_result(false, map_fat_result_to_upload_code(&result), 0)
        }
    }
}

#[inline(never)]
pub(super) async fn handle_remove(
    path: [u8; SD_PATH_MAX],
    path_len: u8,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    if session.is_some() {
        return upload_result(false, SdUploadResultCode::Busy, 0);
    }

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        return upload_result(false, code, 0);
    }

    if let Err(code) = parse_upload_path(&path, path_len) {
        return upload_result(false, code, 0);
    }

    let mut output = [];
    let result = super::super::engine_driver::run_fat_request(
        FatRequest::Remove { path, path_len },
        sd_probe,
        fat_engine,
        &[],
        &mut output,
    )
    .await;
    match sdcard::upload::normalize_path_result(sdcard::upload::PathOperation::Remove, result) {
        FatResult::Done => upload_result(true, SdUploadResultCode::Ok, 0),
        result => upload_result(false, map_fat_result_to_upload_code(&result), 0),
    }
}

#[inline(never)]
pub(super) async fn handle_stat(
    path: [u8; SD_PATH_MAX],
    path_len: u8,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    if session.is_some() {
        return upload_result(false, SdUploadResultCode::Busy, 0);
    }

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        return upload_result(false, code, 0);
    }

    if let Err(code) = parse_upload_path(&path, path_len) {
        return upload_result(false, code, 0);
    }

    let mut output = [];
    let result = super::super::engine_driver::run_fat_request(
        FatRequest::Stat { path, path_len },
        sd_probe,
        fat_engine,
        &[],
        &mut output,
    )
    .await;
    match result {
        FatResult::Stat(entry) => upload_result(true, SdUploadResultCode::Ok, entry.size),
        result => upload_result(false, map_fat_result_to_upload_code(&result), 0),
    }
}

pub(super) fn build_temp_upload_path(
    final_path: &[u8],
) -> Result<([u8; SD_UPLOAD_PATH_BUF_MAX], usize), SdUploadResultCode> {
    sdcard::upload::temporary_path::<SD_UPLOAD_PATH_BUF_MAX>(
        final_path,
        SD_UPLOAD_ROOT,
        SD_UPLOAD_TMP_BASENAME,
    )
    .map_err(|_| SdUploadResultCode::InvalidPath)
}

pub(super) fn parse_upload_path(path: &[u8], path_len: u8) -> Result<&str, SdUploadResultCode> {
    super::helpers::parse_upload_path(path, path_len)
}
