use embassy_net::tcp::TcpSocket;

use super::{RequestContext, RequestRouteKind};

mod basic;
mod params;
mod upload;

pub(super) async fn dispatch_request<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &[u8],
    request: &RequestContext<'_>,
) -> Result<RequestRouteKind, &'static str> {
    let (route_kind, outcome) = match (request.method, request.request_path) {
        ("GET", "/health") => (
            RequestRouteKind::Health,
            basic::handle_health::<H>(socket, request).await,
        ),
        ("GET", "/stat") => (
            RequestRouteKind::Stat,
            basic::handle_stat::<H>(socket, request).await,
        ),
        ("POST", "/mkdir") => (
            RequestRouteKind::Mkdir,
            basic::handle_mkdir::<H>(socket, request).await,
        ),
        ("DELETE", "/rm") => (
            RequestRouteKind::Remove,
            basic::handle_delete::<H>(socket, request).await,
        ),
        ("POST", "/upload_begin") => (
            RequestRouteKind::UploadBegin,
            upload::handle_upload_begin::<H>(socket, request).await,
        ),
        ("PUT", "/upload_chunk") => (
            RequestRouteKind::UploadChunk,
            upload::handle_upload_chunk::<H>(socket, chunk_buf, header_buf, request).await,
        ),
        ("POST", "/upload_commit") => (
            RequestRouteKind::UploadCommit,
            upload::handle_upload_commit::<H>(socket, request).await,
        ),
        ("POST", "/upload_abort") => (
            RequestRouteKind::UploadAbort,
            upload::handle_upload_abort::<H>(socket, request).await,
        ),
        ("PUT", "/upload") => (
            RequestRouteKind::Upload,
            upload::handle_upload::<H>(socket, chunk_buf, header_buf, request).await,
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

async fn write_success_response(
    socket: &mut impl embedded_io_async::Write,
    route: RequestRouteKind,
) {
    let (status, body): (&[u8], &[u8]) = match route {
        RequestRouteKind::Health => (b"200 OK", b"ok"),
        RequestRouteKind::Stat => (b"200 OK", b"stat ok"),
        RequestRouteKind::Mkdir => (b"200 OK", b"mkdir ok"),
        RequestRouteKind::Remove => (b"200 OK", b"delete ok"),
        RequestRouteKind::UploadBegin => (b"200 OK", b"begin ok"),
        RequestRouteKind::UploadChunk => (b"200 OK", b"chunk ok"),
        RequestRouteKind::UploadCommit => (b"200 OK", b"commit ok"),
        RequestRouteKind::UploadAbort => (b"200 OK", b"abort ok"),
        RequestRouteKind::Upload => (b"201 Created", b"upload ok"),
        RequestRouteKind::NotFound => (b"404 Not Found", b"not found"),
    };
    params::write_response(socket, status, body).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;
    struct Output(Vec<u8>);
    impl embedded_io_async::ErrorType for Output {
        type Error = core::convert::Infallible;
    }
    impl embedded_io_async::Write for Output {
        async fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        async fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }
    #[test]
    fn direct_and_staged_success_responses_preserve_wire_contract() {
        let cases = [
            (RequestRouteKind::Health, "200 OK", "ok"),
            (RequestRouteKind::Stat, "200 OK", "stat ok"),
            (RequestRouteKind::Mkdir, "200 OK", "mkdir ok"),
            (RequestRouteKind::Remove, "200 OK", "delete ok"),
            (RequestRouteKind::UploadBegin, "200 OK", "begin ok"),
            (RequestRouteKind::UploadChunk, "200 OK", "chunk ok"),
            (RequestRouteKind::UploadCommit, "200 OK", "commit ok"),
            (RequestRouteKind::UploadAbort, "200 OK", "abort ok"),
            (RequestRouteKind::Upload, "201 Created", "upload ok"),
        ];
        for (route, status, body) in cases {
            let mut output = Output(Vec::new());
            {
                let future = write_success_response(&mut output, route);
                let mut future = core::pin::pin!(future);
                let mut context = core::task::Context::from_waker(core::task::Waker::noop());
                assert!(core::future::Future::poll(future.as_mut(), &mut context).is_ready());
            }
            assert_eq!(output.0,std::format!("HTTP/1.1 {status}\r\nConnection: keep-alive\r\nContent-Length: {}\r\n\r\n{body}",body.len()).as_bytes());
        }
    }
}
