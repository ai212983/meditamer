use crate::firmware::telemetry;
use embassy_time::Instant;
use sdcard::fat::{FatEngine, FatRequest, FatResult};

use super::{
    build_temp_upload_path, copy_fat_path, ensure_upload_ready, map_fat_result_to_upload_code,
    parse_upload_path, upload_result, SdProbeDriver, SdUploadBegin, SdUploadResult,
    SdUploadResultCode, SdUploadSession, SD_UPLOAD_PATH_BUF_MAX,
};

#[inline(never)]
pub(super) async fn handle_begin(
    begin: SdUploadBegin,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    let SdUploadBegin {
        path,
        path_len,
        expected_size,
    } = begin;
    telemetry::log_stack_headroom("sd_upload_begin_entry");
    if session.is_some() {
        return upload_result(false, SdUploadResultCode::Busy, 0);
    }

    let final_path = match parse_upload_path(&path, path_len) {
        Ok(path) => path,
        Err(code) => return upload_result(false, code, 0),
    };
    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_SD) {
        esp_println::println!(
            "sd_upload: begin path={} expected_size={}",
            final_path,
            expected_size
        );
    }
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
    let (fat_temp_path, fat_temp_path_len) = match copy_fat_path(&temp_path[..temp_len]) {
        Ok(path) => path,
        Err(code) => return upload_result(false, code, 0),
    };

    telemetry::log_stack_headroom("sd_upload_begin_fat_before");
    let mut output = [];
    let result = super::super::super::engine_driver::run_fat_request(
        FatRequest::UploadBegin {
            path: fat_temp_path,
            path_len: fat_temp_path_len,
            expected_size,
        },
        sd_probe,
        fat_engine,
        &[],
        &mut output,
    )
    .await;
    if !matches!(result, FatResult::Done) {
        esp_println::println!("sd_upload: begin engine failed result={:?}", result);
        return upload_result(false, map_fat_result_to_upload_code(&result), 0);
    }
    telemetry::log_stack_headroom("sd_upload_begin_fat_after");
    let mut final_path_buf = [0u8; SD_UPLOAD_PATH_BUF_MAX];
    final_path_buf[..final_path_bytes.len()].copy_from_slice(final_path_bytes);
    *session = Some(SdUploadSession {
        final_path: final_path_buf,
        final_path_len: final_path_bytes.len() as u8,
        temp_path,
        temp_path_len: temp_len as u8,
        expected_size,
        bytes_written: 0,
        last_activity_at: Instant::now(),
        write_metrics_start: sd_probe.write_metrics_snapshot(),
        chunk_timing: super::super::types::SdUploadChunkTimingMetrics::default(),
    });
    upload_result(true, SdUploadResultCode::Ok, 0)
}
