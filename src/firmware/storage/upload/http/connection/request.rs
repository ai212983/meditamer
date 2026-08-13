use embassy_net::tcp::{Error as TcpError, TcpSocket};
use esp_println::println;

use super::super::helpers::{
    drain_remaining_body, find_header_end, parse_content_length, validate_upload_auth,
    write_response, UploadAuthError,
};
use super::RequestContext;
use crate::firmware::observability;

pub(super) async fn authorize_request(
    socket: &mut TcpSocket<'_>,
    header: &str,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    // Contract: /health must remain unauthenticated so host-side reachability
    // checks can distinguish auth failures from transport/listener regressions.
    if request.request_path == "/health" {
        return Ok(());
    }

    match validate_upload_auth(header) {
        Ok(()) => Ok(()),
        Err(UploadAuthError::MissingOrInvalidToken) => {
            drain_body(socket, request).await?;
            write_response(
                socket,
                b"401 Unauthorized",
                b"missing or invalid upload token",
            )
            .await;
            Err("missing or invalid upload token")
        }
    }
}

async fn drain_body(
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

pub(super) async fn read_header(
    socket: &mut TcpSocket<'_>,
    header_buf: &mut [u8],
) -> Result<(usize, usize), &'static str> {
    let mut filled = 0usize;
    let mut header_read_ops = 0u32;

    let header_end = loop {
        if filled == header_buf.len() {
            write_response(socket, b"413 Payload Too Large", b"header too large").await;
            return Err("header too large");
        }

        let n = read_header_chunk(socket, &mut header_buf[filled..], filled).await?;

        if n == 0 {
            return log_header_eof(socket, filled, header_read_ops);
        }

        header_read_ops = header_read_ops.saturating_add(1);
        filled += n;

        if let Some(end) = find_header_end(&header_buf[..filled]) {
            break end;
        }
    };

    Ok((filled, header_end))
}

async fn read_header_chunk(
    socket: &mut TcpSocket<'_>,
    buffer: &mut [u8],
    filled: usize,
) -> Result<usize, &'static str> {
    match socket.read(buffer).await {
        Ok(n) => Ok(n),
        Err(err) => {
            log_header_read_error(socket, filled, err);
            if matches!(err, TcpError::ConnectionReset) && filled == 0 {
                Err("read header reset empty")
            } else if matches!(err, TcpError::ConnectionReset) {
                Err("read header reset")
            } else if filled == 0 {
                Err("read header empty")
            } else {
                Err("read header")
            }
        }
    }
}

fn log_header_read_error(socket: &TcpSocket<'_>, filled: usize, err: TcpError) {
    if observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
        println!(
            "upload_http: header read err={:?} filled={} recv_queue={} send_queue={} state={:?} remote={:?}",
            err,
            filled,
            socket.recv_queue(),
            socket.send_queue(),
            socket.state(),
            socket.remote_endpoint(),
        );
    }
}

fn log_header_eof(
    socket: &TcpSocket<'_>,
    filled: usize,
    header_read_ops: u32,
) -> Result<(usize, usize), &'static str> {
    if observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
        println!(
            "upload_http: header eof filled={} reads={} recv_queue={} state={:?} remote={:?}",
            filled,
            header_read_ops,
            socket.recv_queue(),
            socket.state(),
            socket.remote_endpoint(),
        );
    }
    Err("eof header")
}

pub(super) async fn parse_content_length_or_http_error(
    socket: &mut TcpSocket<'_>,
    header: &str,
) -> Result<Option<usize>, &'static str> {
    match parse_content_length(header) {
        Ok(value) => Ok(value),
        Err(err) => {
            write_response(socket, b"400 Bad Request", b"invalid Content-Length").await;
            Err(err)
        }
    }
}

pub(super) fn connection_close_requested(header: &str) -> bool {
    for line in header.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.eq_ignore_ascii_case("connection") {
            continue;
        }
        if value
            .split(',')
            .any(|token| token.trim().eq_ignore_ascii_case("close"))
        {
            return true;
        }
    }
    false
}
