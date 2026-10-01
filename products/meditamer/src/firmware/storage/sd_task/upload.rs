#[cfg(feature = "asset-upload-http")]
use embassy_time::Instant;
use sdcard::fat::FatEngine;

#[cfg(all(test, not(target_os = "none")))]
use super::super::super::types::SdUploadResultCode;
use super::super::super::types::{SdProbeDriver, SdUploadRequest, SdUploadResult};

mod dispatch;
#[cfg(feature = "asset-upload-http")]
mod fence;
mod helpers;
mod metrics;
mod path_ops;
mod stream;
mod types;

#[cfg(feature = "asset-upload-http")]
pub(super) use fence::{no_session_abort_fence_result, route_upload_request, UploadRequestRoute};
pub(super) use types::SdUploadSession;

pub(super) async fn process_upload_request(
    request: SdUploadRequest,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    dispatch::process_upload_request(
        request,
        session,
        sd_probe,
        powered,
        upload_mounted,
        fat_engine,
    )
    .await
}

#[cfg(all(test, not(target_os = "none")))]
pub(super) fn build_temp_upload_path(
    final_path: &[u8],
) -> Result<([u8; super::SD_UPLOAD_PATH_BUF_MAX], usize), SdUploadResultCode> {
    path_ops::build_temp_upload_path(final_path)
}

#[cfg(all(test, not(target_os = "none")))]
pub(super) fn parse_upload_path(path: &[u8], path_len: u8) -> Result<&str, SdUploadResultCode> {
    path_ops::parse_upload_path(path, path_len)
}

#[cfg(feature = "asset-upload-http")]
pub(super) fn active_session_last_activity(session: &Option<SdUploadSession>) -> Option<Instant> {
    session
        .as_ref()
        .map(|active| active.core.last_activity_at())
}
