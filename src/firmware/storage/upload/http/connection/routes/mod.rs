use embassy_net::tcp::TcpSocket;

use super::{RequestContext, RequestRouteKind};

mod basic;
mod params;
mod upload;

pub(super) async fn dispatch_request(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &[u8],
    request: &RequestContext<'_>,
) -> Result<RequestRouteKind, &'static str> {
    let (route_kind, outcome) = match (request.method, request.request_path) {
        ("GET", "/health") => (
            RequestRouteKind::Health,
            basic::handle_health(socket, request).await,
        ),
        ("GET", "/stat") => (
            RequestRouteKind::Stat,
            basic::handle_stat(socket, request).await,
        ),
        ("POST", "/mkdir") => (
            RequestRouteKind::Mkdir,
            basic::handle_mkdir(socket, request).await,
        ),
        ("DELETE", "/rm") => (
            RequestRouteKind::Remove,
            basic::handle_delete(socket, request).await,
        ),
        ("POST", "/upload_begin") => (
            RequestRouteKind::UploadBegin,
            upload::handle_upload_begin(socket, request).await,
        ),
        ("PUT", "/upload_chunk") => (
            RequestRouteKind::UploadChunk,
            upload::handle_upload_chunk(socket, chunk_buf, header_buf, request).await,
        ),
        ("POST", "/upload_commit") => (
            RequestRouteKind::UploadCommit,
            upload::handle_upload_commit(socket, request).await,
        ),
        ("POST", "/upload_abort") => (
            RequestRouteKind::UploadAbort,
            upload::handle_upload_abort(socket, request).await,
        ),
        ("PUT", "/upload") => (
            RequestRouteKind::Upload,
            upload::handle_upload(socket, chunk_buf, header_buf, request).await,
        ),
        _ => (
            RequestRouteKind::NotFound,
            handle_not_found(socket, request).await,
        ),
    };
    outcome.map(|_| route_kind)
}

async fn handle_not_found(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    params::drain_body(socket, request).await?;
    params::write_response(socket, b"404 Not Found", b"not found").await;
    Ok(())
}
