use core::cmp::min;

use embassy_net::tcp::TcpSocket;

use super::super::super::helpers::{
    drain_remaining_body, parse_path_query, parse_u32_query,
};
use super::super::SdPath;
use super::RequestContext;

pub(super) use super::super::super::helpers::{
    sd_upload_or_http_error, write_response, write_roundtrip_error_response,
};

pub(super) async fn drain_body(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_remaining_body(
        socket,
        request.content_length_or_zero,
        request.body_bytes_in_buffer,
    )
    .await
}

pub(super) async fn parse_path_or_400(
    socket: &mut TcpSocket<'_>,
    target: &str,
    route: &str,
) -> Result<SdPath, &'static str> {
    match parse_path_query(target, route) {
        Ok(path) => Ok(path),
        Err(err) => {
            write_response(socket, b"400 Bad Request", b"invalid path query").await;
            Err(err)
        }
    }
}

pub(super) async fn parse_u32_or_400(
    socket: &mut TcpSocket<'_>,
    target: &str,
    route: &str,
    key: &str,
) -> Result<u32, &'static str> {
    match parse_u32_query(target, route, key) {
        Ok(value) => Ok(value),
        Err(err) => {
            write_response(socket, b"400 Bad Request", b"invalid size query").await;
            Err(err)
        }
    }
}

pub(super) async fn required_content_length(
    socket: &mut TcpSocket<'_>,
    content_length: Option<usize>,
) -> Result<usize, &'static str> {
    match content_length {
        Some(value) => Ok(value),
        None => {
            write_response(socket, b"411 Length Required", b"Content-Length required").await;
            Err("missing content-length")
        }
    }
}

pub(super) fn prefetched_body_slice<'a>(
    header_buf: &'a [u8],
    request: &RequestContext<'_>,
    content_length: usize,
) -> &'a [u8] {
    let prefetched_len = min(request.body_bytes_in_buffer, content_length);
    &header_buf[request.body_start..request.body_start + prefetched_len]
}
