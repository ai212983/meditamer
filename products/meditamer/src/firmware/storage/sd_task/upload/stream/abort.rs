use sdcard::fat::{FatEngine, FatResult, SdFatError};

use super::{
    ensure_upload_ready, map_fat_result_to_upload_code, upload_result, SdProbeDriver,
    SdUploadResult, SdUploadResultCode, SdUploadSession,
};

#[inline(never)]
pub(super) async fn handle_abort(
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    let Some(active) = session.take() else {
        return upload_result(true, SdUploadResultCode::Ok, 0);
    };
    let bytes_written = active.core.bytes_written();

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        console::println!(
            "sd_upload: abort ensure_upload_ready failed code={:?} bytes_written={}",
            code,
            bytes_written
        );
        return upload_result(false, code, bytes_written);
    }

    let (temp_path_buf, temp_path_len) = active.core.temp_path();
    let temp_bytes = &temp_path_buf[..temp_path_len as usize];
    let temp_path_str = core::str::from_utf8(temp_bytes).unwrap_or("<invalid>");
    let abort_requests = match active.core.abort_requests() {
        Ok(requests) => requests,
        Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
    };
    let mut output = [];
    let remove = super::super::super::engine_driver::run_fat_request(
        abort_requests.remove,
        sd_probe,
        fat_engine,
        &[],
        &mut output,
    )
    .await;
    let _ = super::super::super::engine_driver::run_fat_request(
        abort_requests.clear,
        sd_probe,
        fat_engine,
        &[],
        &mut output,
    )
    .await;
    match remove {
        FatResult::Done
        | FatResult::Error(sdcard::fat::FatEngineError::Fat(SdFatError::NotFound)) => {
            upload_result(true, SdUploadResultCode::Ok, bytes_written)
        }
        result => {
            console::println!(
                "sd_upload: abort remove failed temp_path={} result={:?}",
                temp_path_str,
                result
            );
            upload_result(false, map_fat_result_to_upload_code(&result), bytes_written)
        }
    }
}
