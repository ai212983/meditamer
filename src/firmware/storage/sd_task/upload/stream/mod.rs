use super::super::super::super::types::{
    SdProbeDriver, SdUploadResult, SdUploadResultCode, SD_PATH_MAX, SD_UPLOAD_CHUNK_MAX,
};
use super::super::SD_UPLOAD_PATH_BUF_MAX;
use super::helpers::{
    copy_fat_path, ensure_upload_ready, map_fat_result_to_upload_code, upload_result,
};
use super::metrics::write_metrics_delta;
use super::path_ops::{build_temp_upload_path, parse_upload_path};
use super::types::{SdUploadChunkTimingSample, SdUploadSession};
use embassy_time::Instant;
use sdcard::fat::FatEngine;

mod begin;
mod chunk;
mod finish;

pub(super) struct SdUploadBegin {
    pub(super) path: [u8; SD_PATH_MAX],
    pub(super) path_len: u8,
    pub(super) expected_size: u32,
}

pub(super) async fn handle_begin(
    begin: SdUploadBegin,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    begin::handle_begin(
        begin,
        session,
        sd_probe,
        powered,
        upload_mounted,
        fat_engine,
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
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    chunk::handle_chunk(
        data_len,
        queue_wait_ms,
        session,
        sd_probe,
        powered,
        upload_mounted,
        fat_engine,
    )
    .await
}

pub(super) async fn handle_commit(
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    finish::handle_commit(session, sd_probe, powered, upload_mounted, fat_engine).await
}

pub(super) async fn handle_abort(
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    finish::handle_abort(session, sd_probe, powered, upload_mounted, fat_engine).await
}

fn div_or_zero(total: u32, count: u32) -> u32 {
    total.checked_div(count).unwrap_or(0)
}

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}
