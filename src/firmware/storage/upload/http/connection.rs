use embassy_net::tcp::TcpSocket;
use embassy_time::Duration;
use esp_println::println;

use super::super::super::super::telemetry;
use super::super::super::super::types::SD_PATH_MAX;
use super::helpers::target_path;

mod body;
mod fairness;
mod request;
mod routes;

pub(super) const HTTP_HEADER_READ_TIMEOUT_MS: u64 = 10_000;
pub(super) const HTTP_HEADER_KEEPALIVE_IDLE_TIMEOUT_MS: u64 = 500;
pub(super) const HTTP_UPLOAD_BODY_READ_TIMEOUT_MS: u64 = 6_000;
pub type SdPath = ([u8; SD_PATH_MAX], u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RequestRouteKind {
    Health,
    Stat,
    Mkdir,
    Remove,
    UploadBegin,
    UploadChunk,
    UploadCommit,
    UploadAbort,
    Upload,
    NotFound,
}

impl RequestRouteKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Health => "health",
            Self::Stat => "stat",
            Self::Mkdir => "mkdir",
            Self::Remove => "rm",
            Self::UploadBegin => "upload_begin",
            Self::UploadChunk => "upload_chunk",
            Self::UploadCommit => "upload_commit",
            Self::UploadAbort => "upload_abort",
            Self::Upload => "upload",
            Self::NotFound => "not_found",
        }
    }
}

pub(super) struct RequestContext<'a> {
    method: &'a str,
    target: &'a str,
    request_path: &'a str,
    content_length: Option<usize>,
    content_length_or_zero: usize,
    body_start: usize,
    body_bytes_in_buffer: usize,
}

pub(super) struct HandledRequest {
    pub(super) route_kind: RequestRouteKind,
    pub(super) connection_close_requested: bool,
}

pub(super) async fn handle_connection(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &mut [u8],
    header_timeout_ms: u64,
) -> Result<HandledRequest, &'static str> {
    socket.set_timeout(Some(Duration::from_millis(header_timeout_ms)));
    let header_result = request::read_header(socket, header_buf).await;
    socket.set_timeout(Some(Duration::from_secs(super::HTTP_SOCKET_TIMEOUT_SECS)));
    let (filled, header_end) = header_result?;
    let header = core::str::from_utf8(&header_buf[..header_end]).map_err(|_| "header utf8")?;
    let connection_close_requested = request::connection_close_requested(header);
    let (method, target) = super::helpers::parse_request_line(header).ok_or("bad request line")?;
    let content_length = request::parse_content_length_or_http_error(socket, header).await?;
    let request = RequestContext {
        method,
        target,
        request_path: target_path(target),
        content_length,
        content_length_or_zero: content_length.unwrap_or(0),
        body_start: header_end + 4,
        body_bytes_in_buffer: filled.saturating_sub(header_end + 4),
    };

    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
        println!(
            "upload_http: request method={} path={}",
            request.method, request.request_path
        );
    }

    request::authorize_request(socket, header, &request).await?;
    let route_kind = routes::dispatch_request(socket, chunk_buf, header_buf, &request).await?;
    Ok(HandledRequest {
        route_kind,
        connection_close_requested,
    })
}
