//! Asset upload over HTTP.
//!
//! [`http`] serves the upload protocol; [`sd_bridge`] turns its routes into SD
//! commands. The radio and the network stack belong to [`crate::firmware::net`];
//! this module is one of its consumers.

mod http;
mod sd_bridge;

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_net::Stack;

static SD_UPLOAD_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) async fn run_http_server(stack: Stack<'_>) {
    http::run_http_server(stack).await;
}

pub(crate) fn active_http_connections() -> u16 {
    http::active_connections()
}

pub(crate) fn active_sd_roundtrips() -> u16 {
    sd_bridge::active_roundtrips()
}

pub(crate) fn sd_upload_session_active() -> bool {
    SD_UPLOAD_SESSION_ACTIVE.load(Ordering::Acquire)
}

pub(crate) fn set_sd_upload_session_active(active: bool) {
    SD_UPLOAD_SESSION_ACTIVE.store(active, Ordering::Release);
}

pub(crate) async fn abort_sd_upload() -> bool {
    use crate::firmware::types::SdUploadResultCode;
    matches!(
        sd_bridge::sd_upload_roundtrip(crate::firmware::types::SdUploadCommand::Abort).await,
        Ok(_)
            | Err(sd_bridge::SdUploadRoundtripError::Device(
                SdUploadResultCode::SessionNotActive
            ))
    )
}
