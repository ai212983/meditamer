//! Stream mirror: real Abort handler plus test drivers.

use super::super::super::super::types::{SdProbeDriver, SdUploadResult, SdUploadResultCode};
use super::helpers::{ensure_upload_ready, map_fat_result_to_upload_code, upload_result};
use super::types::SdUploadSession;
use sdcard::fat::FatEngine;
use sdcard::probe::SdWriteMetrics;

#[path = "../../../../../products/meditamer/src/firmware/storage/sd_task/upload/stream/abort.rs"]
mod abort;

/// Thin driver delegating to the real Abort handler; it adds no behavior,
/// only bridges prod `pub(super)` visibility to the tests.
pub(super) async fn drive_abort(
    session: &mut Option<SdUploadSession>,
    probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    engine: &mut FatEngine,
) -> SdUploadResult {
    abort::handle_abort(session, probe, powered, upload_mounted, engine).await
}

/// Opens a real upload session via the production state machine, mirroring
/// a begun transfer awaiting cleanup.
pub(super) fn open_upload_session(
    final_path: &[u8],
    temp_path: &[u8],
    expected_size: u32,
) -> Option<SdUploadSession> {
    let core = sdcard::upload::Session::<{ super::super::SD_UPLOAD_PATH_BUF_MAX }>::begin(
        final_path,
        temp_path,
        expected_size,
    )
    .expect("test session begin");
    Some(SdUploadSession {
        core,
        write_metrics_start: SdWriteMetrics,
        chunk_timing: Default::default(),
    })
}

pub(super) fn session_accept_chunk(session: &mut Option<SdUploadSession>, length: u32) {
    session
        .as_mut()
        .expect("test session present")
        .core
        .accept_chunk(length)
        .expect("test chunk fits");
}
