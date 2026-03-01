use embassy_time::Instant;

use super::super::super::types::{
    SdProbeDriver, SdUploadRequest, SdUploadResult, SdUploadResultCode,
};

mod dispatch;
mod helpers;
mod metrics;
mod path_ops;
mod stream;
mod types;

pub(super) use types::SdUploadSession;

pub(super) async fn process_upload_request(
    request: SdUploadRequest,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    dispatch::process_upload_request(request, session, sd_probe, powered, upload_mounted).await
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

pub(super) fn active_session_last_activity(session: &Option<SdUploadSession>) -> Option<Instant> {
    session.as_ref().map(|active| active.last_activity_at)
}

pub(super) async fn ensure_upload_ready(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> Result<(), SdUploadResultCode> {
    helpers::ensure_upload_ready(sd_probe, powered, upload_mounted).await
}
