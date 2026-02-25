use core::cmp::min;

use embassy_net::tcp::TcpSocket;
use embedded_io_async::Write;

use super::super::sd_bridge::{
    roundtrip_error_body, roundtrip_error_log, roundtrip_error_status, sd_upload_roundtrip,
    SdUploadRoundtripError,
};
use super::{UPLOAD_HTTP_ROOT, UPLOAD_HTTP_TOKEN_HEADER};
use crate::firmware::types::{SdUploadCommand, SD_PATH_MAX};

#[derive(Copy, Clone)]
pub(super) enum UploadAuthError {
    MissingOrInvalidToken,
}

pub(super) async fn sd_upload_or_http_error(
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

pub(super) async fn write_roundtrip_error_response(
    socket: &mut TcpSocket<'_>,
    error: SdUploadRoundtripError,
) {
    write_response(
        socket,
        roundtrip_error_status(error),
        roundtrip_error_body(error),
    )
    .await;
}

pub(super) async fn drain_remaining_body(
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

pub(super) fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

pub(super) fn parse_request_line(header: &str) -> Option<(&str, &str)> {
    let first_line = header.lines().next()?;
    let mut parts = first_line.split_ascii_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    let _version = parts.next()?;
    Some((method, target))
}

pub(super) fn parse_content_length(header: &str) -> Result<Option<usize>, &'static str> {
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

pub(super) fn target_path(target: &str) -> &str {
    target.split('?').next().unwrap_or(target)
}

pub(super) fn validate_upload_auth(header: &str) -> Result<(), UploadAuthError> {
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

pub(super) fn parse_path_query(
    target: &str,
    route: &str,
) -> Result<([u8; SD_PATH_MAX], u8), &'static str> {
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

pub(super) fn parse_u32_query(target: &str, route: &str, key: &str) -> Result<u32, &'static str> {
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

pub(super) async fn write_response(socket: &mut TcpSocket<'_>, status: &[u8], body: &[u8]) {
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
