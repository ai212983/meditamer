use core::cmp::min;

use embassy_net::{tcp::TcpSocket, IpListenEndpoint, Stack};
use embassy_time::{with_timeout, Duration};
use embedded_io_async::Write;
use static_cell::StaticCell;

use super::super::types::{SdUploadCommand, SD_PATH_MAX, SD_UPLOAD_CHUNK_MAX};
use super::sd_bridge::{
    roundtrip_error_body, roundtrip_error_log, roundtrip_error_status, sd_upload_chunk,
    sd_upload_roundtrip, SdUploadRoundtripError,
};

const UPLOAD_HTTP_PORT: u16 = 8080;
const UPLOAD_HTTP_ROOT: &str = "/assets";
const UPLOAD_HTTP_TOKEN_HEADER: &str = "x-upload-token";
const HTTP_HEADER_MAX: usize = 2048;
const HTTP_RW_BUF: usize = 2048;

#[derive(Copy, Clone)]
enum UploadAuthError {
    MissingOrInvalidToken,
}

pub(super) async fn run_http_server(stack: Stack<'static>) {
    static RX_BUFFER: StaticCell<[u8; HTTP_RW_BUF]> = StaticCell::new();
    static TX_BUFFER: StaticCell<[u8; HTTP_RW_BUF]> = StaticCell::new();
    static CHUNK_BUFFER: StaticCell<[u8; SD_UPLOAD_CHUNK_MAX]> = StaticCell::new();

    let rx_buffer = RX_BUFFER.init([0u8; HTTP_RW_BUF]);
    let tx_buffer = TX_BUFFER.init([0u8; HTTP_RW_BUF]);
    let chunk_buffer = CHUNK_BUFFER.init([0u8; SD_UPLOAD_CHUNK_MAX]);

    stack.wait_config_up().await;
    if let Some(cfg) = stack.config_v4() {
        esp_println::println!(
            "upload_http: listening on {}:{}",
            cfg.address.address(),
            UPLOAD_HTTP_PORT
        );
    }

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer[..], &mut tx_buffer[..]);
        socket.set_timeout(Some(Duration::from_secs(20)));

        let accepted = socket
            .accept(IpListenEndpoint {
                addr: None,
                port: UPLOAD_HTTP_PORT,
            })
            .await;
        if let Err(err) = accepted {
            esp_println::println!("upload_http: accept err={:?}", err);
            continue;
        }

        if let Err(err) = handle_connection(&mut socket, chunk_buffer).await {
            esp_println::println!("upload_http: request err={}", err);
        }

        let _ = with_timeout(Duration::from_millis(250), socket.flush()).await;
        socket.close();
    }
}

async fn handle_connection(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8; SD_UPLOAD_CHUNK_MAX],
) -> Result<(), &'static str> {
    let mut header_buf = [0u8; HTTP_HEADER_MAX];
    let mut filled = 0usize;
    let header_end = loop {
        if filled == header_buf.len() {
            write_response(socket, b"413 Payload Too Large", b"header too large").await;
            return Err("header too large");
        }

        let n = socket
            .read(&mut header_buf[filled..])
            .await
            .map_err(|_| "read")?;
        if n == 0 {
            return Err("eof");
        }
        filled += n;

        if let Some(end) = find_header_end(&header_buf[..filled]) {
            break end;
        }
    };

    let header = core::str::from_utf8(&header_buf[..header_end]).map_err(|_| "header utf8")?;
    let (method, target) = parse_request_line(header).ok_or("bad request line")?;
    let content_length = match parse_content_length(header) {
        Ok(value) => value,
        Err(err) => {
            write_response(socket, b"400 Bad Request", b"invalid Content-Length").await;
            return Err(err);
        }
    };
    let content_length_or_zero = content_length.unwrap_or(0);
    let body_start = header_end + 4;
    let body_bytes_in_buffer = filled.saturating_sub(body_start);
    let request_path = target_path(target);

    if request_path != "/health" {
        match validate_upload_auth(header) {
            Ok(()) => {}
            Err(UploadAuthError::MissingOrInvalidToken) => {
                drain_remaining_body(socket, content_length_or_zero, body_bytes_in_buffer).await?;
                write_response(
                    socket,
                    b"401 Unauthorized",
                    b"missing or invalid upload token",
                )
                .await;
                return Err("missing or invalid upload token");
            }
        }
    }

    match (method, request_path) {
        ("GET", "/health") => {
            drain_remaining_body(socket, content_length_or_zero, body_bytes_in_buffer).await?;
            write_response(socket, b"200 OK", b"ok").await;
            Ok(())
        }
        ("POST", "/mkdir") => {
            drain_remaining_body(socket, content_length_or_zero, body_bytes_in_buffer).await?;
            let (path, path_len) = match parse_path_query(target, "/mkdir") {
                Ok(path) => path,
                Err(err) => {
                    write_response(socket, b"400 Bad Request", b"invalid path query").await;
                    return Err(err);
                }
            };
            sd_upload_or_http_error(socket, SdUploadCommand::Mkdir { path, path_len }).await?;
            write_response(socket, b"200 OK", b"mkdir ok").await;
            Ok(())
        }
        ("DELETE", "/rm") => {
            drain_remaining_body(socket, content_length_or_zero, body_bytes_in_buffer).await?;
            let (path, path_len) = match parse_path_query(target, "/rm") {
                Ok(path) => path,
                Err(err) => {
                    write_response(socket, b"400 Bad Request", b"invalid path query").await;
                    return Err(err);
                }
            };
            sd_upload_or_http_error(socket, SdUploadCommand::Remove { path, path_len }).await?;
            write_response(socket, b"200 OK", b"delete ok").await;
            Ok(())
        }
        ("POST", "/upload_begin") => {
            drain_remaining_body(socket, content_length_or_zero, body_bytes_in_buffer).await?;
            let (path, path_len) = match parse_path_query(target, "/upload_begin") {
                Ok(path) => path,
                Err(err) => {
                    write_response(socket, b"400 Bad Request", b"invalid path query").await;
                    return Err(err);
                }
            };
            let expected_size = match parse_u32_query(target, "/upload_begin", "size") {
                Ok(size) => size,
                Err(err) => {
                    write_response(socket, b"400 Bad Request", b"invalid size query").await;
                    return Err(err);
                }
            };
            sd_upload_or_http_error(
                socket,
                SdUploadCommand::Begin {
                    path,
                    path_len,
                    expected_size,
                },
            )
            .await?;
            write_response(socket, b"200 OK", b"begin ok").await;
            Ok(())
        }
        ("PUT", "/upload_chunk") => {
            let content_length = match content_length {
                Some(value) => value,
                None => {
                    write_response(socket, b"411 Length Required", b"Content-Length required")
                        .await;
                    return Err("missing content-length");
                }
            };
            let mut sent = 0usize;
            if body_bytes_in_buffer > 0 {
                let take = min(body_bytes_in_buffer, content_length);
                let mut offset = 0usize;
                while offset < take {
                    let end = min(offset + SD_UPLOAD_CHUNK_MAX, take);
                    let chunk = &header_buf[body_start + offset..body_start + end];
                    if let Err(err) = sd_upload_chunk(chunk).await {
                        write_roundtrip_error_response(socket, err).await;
                        return Err(roundtrip_error_log(err));
                    }
                    sent += chunk.len();
                    offset = end;
                }
            }

            while sent < content_length {
                let want = min(chunk_buf.len(), content_length - sent);
                let n = socket
                    .read(&mut chunk_buf[..want])
                    .await
                    .map_err(|_| "read body")?;
                if n == 0 {
                    write_response(socket, b"400 Bad Request", b"incomplete body").await;
                    return Err("incomplete body");
                }
                if let Err(err) = sd_upload_chunk(&chunk_buf[..n]).await {
                    write_roundtrip_error_response(socket, err).await;
                    return Err(roundtrip_error_log(err));
                }
                sent += n;
            }

            write_response(socket, b"200 OK", b"chunk ok").await;
            Ok(())
        }
        ("POST", "/upload_commit") => {
            drain_remaining_body(socket, content_length_or_zero, body_bytes_in_buffer).await?;
            sd_upload_or_http_error(socket, SdUploadCommand::Commit).await?;
            write_response(socket, b"200 OK", b"commit ok").await;
            Ok(())
        }
        ("POST", "/upload_abort") => {
            drain_remaining_body(socket, content_length_or_zero, body_bytes_in_buffer).await?;
            sd_upload_or_http_error(socket, SdUploadCommand::Abort).await?;
            write_response(socket, b"200 OK", b"abort ok").await;
            Ok(())
        }
        ("PUT", "/upload") => {
            let content_length = match content_length {
                Some(value) => value,
                None => {
                    write_response(socket, b"411 Length Required", b"Content-Length required")
                        .await;
                    return Err("missing content-length");
                }
            };
            let (path, path_len) = match parse_path_query(target, "/upload") {
                Ok(path) => path,
                Err(err) => {
                    write_response(socket, b"400 Bad Request", b"invalid path query").await;
                    return Err(err);
                }
            };
            if content_length > u32::MAX as usize {
                write_response(socket, b"413 Payload Too Large", b"content too large").await;
                return Err("content too large");
            }
            let expected_size = content_length as u32;
            sd_upload_or_http_error(
                socket,
                SdUploadCommand::Begin {
                    path,
                    path_len,
                    expected_size,
                },
            )
            .await?;

            let mut sent = 0usize;
            if body_bytes_in_buffer > 0 {
                let take = min(body_bytes_in_buffer, content_length);
                let mut offset = 0usize;
                while offset < take {
                    let end = min(offset + SD_UPLOAD_CHUNK_MAX, take);
                    let chunk = &header_buf[body_start + offset..body_start + end];
                    if let Err(err) = sd_upload_chunk(chunk).await {
                        let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                        write_roundtrip_error_response(socket, err).await;
                        return Err(roundtrip_error_log(err));
                    }
                    sent += chunk.len();
                    offset = end;
                }
            }

            while sent < content_length {
                let want = min(chunk_buf.len(), content_length - sent);
                let n = socket
                    .read(&mut chunk_buf[..want])
                    .await
                    .map_err(|_| "read body")?;
                if n == 0 {
                    let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                    write_response(socket, b"400 Bad Request", b"incomplete body").await;
                    return Err("incomplete body");
                }
                if let Err(err) = sd_upload_chunk(&chunk_buf[..n]).await {
                    let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                    write_roundtrip_error_response(socket, err).await;
                    return Err(roundtrip_error_log(err));
                }
                sent += n;
            }

            if let Err(err) = sd_upload_roundtrip(SdUploadCommand::Commit).await {
                let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                write_roundtrip_error_response(socket, err).await;
                return Err(roundtrip_error_log(err));
            }
            write_response(socket, b"201 Created", b"upload ok").await;
            Ok(())
        }
        _ => {
            drain_remaining_body(socket, content_length_or_zero, body_bytes_in_buffer).await?;
            write_response(socket, b"404 Not Found", b"not found").await;
            Ok(())
        }
    }
}

async fn sd_upload_or_http_error(
    socket: &mut TcpSocket<'_>,
    command: SdUploadCommand,
) -> Result<(), &'static str> {
    match sd_upload_roundtrip(command).await {
        Ok(_) => Ok(()),
        Err(err) => {
            write_roundtrip_error_response(socket, err).await;
            Err(roundtrip_error_log(err))
        }
    }
}

async fn write_roundtrip_error_response(socket: &mut TcpSocket<'_>, error: SdUploadRoundtripError) {
    write_response(
        socket,
        roundtrip_error_status(error),
        roundtrip_error_body(error),
    )
    .await;
}

async fn drain_remaining_body(
    socket: &mut TcpSocket<'_>,
    content_length: usize,
    already_in_buffer: usize,
) -> Result<(), &'static str> {
    if already_in_buffer >= content_length {
        return Ok(());
    }
    let mut remaining = content_length - already_in_buffer;
    let mut sink = [0u8; 256];
    while remaining > 0 {
        let want = min(remaining, sink.len());
        let n = socket.read(&mut sink[..want]).await.map_err(|_| "drain")?;
        if n == 0 {
            return Err("drain eof");
        }
        remaining -= n;
    }
    Ok(())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_request_line(header: &str) -> Option<(&str, &str)> {
    let first_line = header.lines().next()?;
    let mut parts = first_line.split_ascii_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    let _version = parts.next()?;
    Some((method, target))
}

fn parse_content_length(header: &str) -> Result<Option<usize>, &'static str> {
    let mut content_length = None;

    for line in header.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };

        if !name.eq_ignore_ascii_case("content-length") {
            continue;
        }

        let parsed = value
            .trim()
            .parse::<usize>()
            .map_err(|_| "invalid content-length")?;

        if content_length.is_some() {
            return Err("duplicate content-length");
        }

        content_length = Some(parsed);
    }

    Ok(content_length)
}

fn target_path(target: &str) -> &str {
    target.split('?').next().unwrap_or(target)
}

fn validate_upload_auth(header: &str) -> Result<(), UploadAuthError> {
    let Some(expected_token) =
        option_env!("MEDITAMER_UPLOAD_HTTP_TOKEN").or(option_env!("UPLOAD_HTTP_TOKEN"))
    else {
        // If no compile-time token is configured, treat auth as disabled.
        return Ok(());
    };

    let provided_token = parse_header_value(header, UPLOAD_HTTP_TOKEN_HEADER)
        .ok_or(UploadAuthError::MissingOrInvalidToken)?;

    if provided_token == expected_token {
        Ok(())
    } else {
        Err(UploadAuthError::MissingOrInvalidToken)
    }
}

fn parse_header_value<'a>(header: &'a str, wanted_name: &str) -> Option<&'a str> {
    for line in header.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };

        if name.eq_ignore_ascii_case(wanted_name) {
            return Some(value.trim());
        }
    }

    None
}

fn parse_path_query(target: &str, route: &str) -> Result<([u8; SD_PATH_MAX], u8), &'static str> {
    let query = target
        .strip_prefix(route)
        .and_then(|tail| tail.strip_prefix('?'))
        .ok_or("missing query")?;

    for pair in query.split('&') {
        if let Some(encoded) = pair.strip_prefix("path=") {
            let (path, path_len) = percent_decode_to_path_buf(encoded)?;
            if !path_within_upload_root(&path, path_len) {
                return Err("path outside upload root");
            }
            return Ok((path, path_len));
        }
    }
    Err("missing path query")
}

fn parse_u32_query(target: &str, route: &str, key: &str) -> Result<u32, &'static str> {
    let query = target
        .strip_prefix(route)
        .and_then(|tail| tail.strip_prefix('?'))
        .ok_or("missing query")?;

    for pair in query.split('&') {
        if let Some(value) = pair
            .strip_prefix(key)
            .and_then(|tail| tail.strip_prefix('='))
        {
            return value.parse::<u32>().map_err(|_| "invalid query value");
        }
    }
    Err("missing query key")
}

fn path_within_upload_root(path: &[u8; SD_PATH_MAX], path_len: u8) -> bool {
    let path_len = path_len as usize;
    let path_slice = &path[..path_len];
    let root = UPLOAD_HTTP_ROOT.as_bytes();

    path_slice == root || path_slice.starts_with(root) && path_slice.get(root.len()) == Some(&b'/')
}

fn percent_decode_to_path_buf(encoded: &str) -> Result<([u8; SD_PATH_MAX], u8), &'static str> {
    let mut out = [0u8; SD_PATH_MAX];
    let mut out_len = 0usize;
    let bytes = encoded.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        let decoded = if b == b'%' {
            if i + 2 >= bytes.len() {
                return Err("bad percent-encoding");
            }
            let hi = decode_hex(bytes[i + 1]).ok_or("bad percent-encoding")?;
            let lo = decode_hex(bytes[i + 2]).ok_or("bad percent-encoding")?;
            i += 3;
            (hi << 4) | lo
        } else if b == b'+' {
            i += 1;
            b' '
        } else {
            i += 1;
            b
        };

        if out_len >= out.len() {
            return Err("path too long");
        }
        out[out_len] = decoded;
        out_len += 1;
    }

    if out_len == 0 || out[0] != b'/' {
        return Err("path must be absolute");
    }

    Ok((out, out_len as u8))
}

fn decode_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}

async fn write_response(socket: &mut TcpSocket<'_>, status: &[u8], body: &[u8]) {
    let mut content_length = [0u8; 20];
    let mut idx = content_length.len();
    let mut remaining = body.len();
    loop {
        idx -= 1;
        content_length[idx] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }

    let _ = socket.write_all(b"HTTP/1.0 ").await;
    let _ = socket.write_all(status).await;
    let _ = socket
        .write_all(b"\r\nConnection: close\r\nContent-Length: ")
        .await;
    let _ = socket.write_all(&content_length[idx..]).await;
    let _ = socket.write_all(b"\r\n\r\n").await;
    let _ = socket.write_all(body).await;
}
