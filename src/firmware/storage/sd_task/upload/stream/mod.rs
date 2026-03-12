use super::super::super::super::types::{
    SdProbeDriver, SdUploadResult, SdUploadResultCode, SD_PATH_MAX, SD_UPLOAD_CHUNK_MAX,
};
use super::super::SD_UPLOAD_PATH_BUF_MAX;
use super::helpers::{ensure_upload_ready, map_fat_error_to_upload_code, upload_result};
use super::metrics::write_metrics_delta;
use super::path_ops::{build_temp_upload_path, parse_upload_path};
use super::types::SdUploadSession;
use embassy_time::Instant;

mod begin;
mod chunk;
mod finish;

pub(super) async fn handle_begin(
    path: [u8; SD_PATH_MAX],
    path_len: u8,
    expected_size: u32,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    begin::handle_begin(
        path,
        path_len,
        expected_size,
        session,
        sd_probe,
        powered,
        upload_mounted,
    )
    .await
}

pub(super) async fn handle_chunk(
    data_len: u32,
    queue_wait_ms: u32,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    chunk::handle_chunk(
        data_len,
        queue_wait_ms,
        session,
        sd_probe,
        powered,
        upload_mounted,
    )
    .await
}

pub(super) async fn handle_commit(
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    finish::handle_commit(session, sd_probe, powered, upload_mounted).await
}

pub(super) async fn handle_abort(
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    finish::handle_abort(session, sd_probe, powered, upload_mounted).await
}

fn div_or_zero(total: u32, count: u32) -> u32 {
    if count == 0 {
        0
    } else {
        total / count
    }
}

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}
