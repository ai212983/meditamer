use crate::firmware::telemetry;
use embassy_time::Instant;
use sdcard::fat;

use super::{
    build_temp_upload_path, ensure_upload_ready, map_fat_error_to_upload_code, parse_upload_path,
    upload_result, SdProbeDriver, SdUploadResult, SdUploadResultCode, SdUploadSession,
    SD_PATH_MAX, SD_UPLOAD_PATH_BUF_MAX,
};

#[inline(never)]
pub(super) async fn handle_begin(
    path: [u8; SD_PATH_MAX],
    path_len: u8,
    expected_size: u32,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    telemetry::log_stack_headroom("sd_upload_begin_entry");
    if session.is_some() {
        return upload_result(false, SdUploadResultCode::Busy, 0);
    }

    let final_path = match parse_upload_path(&path, path_len) {
        Ok(path) => path,
        Err(code) => return upload_result(false, code, 0),
    };
    esp_println::println!(
        "sd_upload: begin path={} expected_size={}",
        final_path,
        expected_size
    );
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
    let temp_path_str = match core::str::from_utf8(&temp_path[..temp_len]) {
        Ok(path) => path,
        Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
    };

    telemetry::log_stack_headroom("sd_upload_begin_fat_before");
    let append_session =
        match fat::begin_append_session_create_or_open(sd_probe, temp_path_str).await {
            Ok(session) => session,
            Err(err) => {
                esp_println::println!(
                    "sd_upload: begin append_session_create_or_open failed temp_path={} err={:?}",
                    temp_path_str,
                    err
                );
                return upload_result(false, map_fat_error_to_upload_code(&err), 0);
            }
        };
    telemetry::log_stack_headroom("sd_upload_begin_fat_after");
    let mut final_path_buf = [0u8; SD_UPLOAD_PATH_BUF_MAX];
    final_path_buf[..final_path_bytes.len()].copy_from_slice(final_path_bytes);
    *session = Some(SdUploadSession {
        final_path: final_path_buf,
        final_path_len: final_path_bytes.len() as u8,
        temp_path,
        temp_path_len: temp_len as u8,
        append_session,
        expected_size,
        bytes_written: 0,
        last_activity_at: Instant::now(),
        write_metrics_start: sd_probe.write_metrics_snapshot(),
        chunk_timing: super::super::types::SdUploadChunkTimingMetrics::default(),
    });
    upload_result(true, SdUploadResultCode::Ok, 0)
}
