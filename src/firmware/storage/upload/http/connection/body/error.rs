use embassy_net::tcp::{Error as TcpError, TcpSocket};
use embassy_time::{with_timeout, Duration};
use esp_println::println;

use super::super::super::super::sd_bridge::{
    roundtrip_error_log, sd_upload_roundtrip, SdUploadRoundtripError,
};
use crate::firmware::observability;
use crate::firmware::types::SdUploadCommand;

pub(super) enum UploadBodyError {
    ReadBody {
        err: TcpError,
        consumed: usize,
        content_length: usize,
        pending: usize,
        want: usize,
    },
    IncompleteBody,
    Roundtrip(SdUploadRoundtripError),
}

const UPLOAD_ABORT_RECOVERY_TIMEOUT_MS: u64 = 1_500;

pub(super) fn log_upload_body_read_error(
    socket: &TcpSocket<'_>,
    err: TcpError,
    consumed: usize,
    content_length: usize,
    pending: usize,
    want: usize,
) {
    if observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
        println!(
            "upload_http: body read err={:?} consumed={} of {} pending={} want={} recv_queue={} send_queue={} state={:?} remote={:?}",
            err,
            consumed,
            content_length,
            pending,
            want,
            socket.recv_queue(),
            socket.send_queue(),
            socket.state(),
            socket.remote_endpoint(),
        );
    }
}

pub(super) async fn abort_upload_roundtrip_bounded(reason: &str) {
    let abort_result = with_timeout(
        Duration::from_millis(UPLOAD_ABORT_RECOVERY_TIMEOUT_MS),
        sd_upload_roundtrip(SdUploadCommand::Abort),
    )
    .await;
    if let Ok(Err(err)) = abort_result {
        if observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
            println!(
                "upload_http: abort recovery err={} reason={}",
                roundtrip_error_log(err),
                reason
            );
        }
    }
    if abort_result.is_err() && observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
        println!(
            "upload_http: abort recovery timeout reason={} timeout_ms={}",
            reason, UPLOAD_ABORT_RECOVERY_TIMEOUT_MS
        );
    }
}
