use super::super::super::super::types::{SdUploadCommand, SdUploadResult, SdUploadResultCode};

/// Intake route for one queued upload command, decided before transfer
/// admission and storage readiness.
pub(in super::super) enum UploadRequestRoute {
    /// Supervisor Abort fence. Bypasses the upload-mode and storage-ready
    /// gates; session presence selects administrative acknowledgement
    /// (no session owns temp state) versus real dispatch cleanup.
    AbortFence { session_active: bool },
    /// Ordinary transfer. Still gated by upload mode and storage readiness.
    GatedTransfer,
}

pub(in super::super) fn route_upload_request(
    command: &SdUploadCommand,
    session_active: bool,
) -> UploadRequestRoute {
    match command {
        SdUploadCommand::Abort => UploadRequestRoute::AbortFence { session_active },
        _ => UploadRequestRoute::GatedTransfer,
    }
}

/// Administrative fence result: no session owns temp state, so success is
/// reported without contacting the probe or the FAT engine.
pub(in super::super) fn no_session_abort_fence_result(request_id: u32) -> SdUploadResult {
    SdUploadResult {
        request_id,
        ok: true,
        code: SdUploadResultCode::Ok,
        bytes_written: 0,
        chunk_queue_wait_ms: 0,
        chunk_handler_ms: 0,
        chunk_post_handler_ms: 0,
        chunk_published_at_ms: 0,
        chunk_handler_done_at_ms: 0,
    }
}
