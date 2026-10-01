use core::cmp::min;
use sdcard::SD_PATH_MAX;

use embassy_net::tcp::TcpSocket;
use embedded_io_async::Write;

#[derive(Copy, Clone)]
pub(super) enum UploadAuthError {
    MissingOrInvalidToken,
}

pub(super) async fn sd_upload_or_http_error<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    command: crate::Command,
) -> Result<(), &'static str> {
    match H::roundtrip(command).await {
        Ok(_) => Ok(()),
        Err(err) => {
            write_roundtrip_error_response::<H>(socket, err).await;
            Err(H::error_log(err))
        }
    }
}

pub(super) async fn write_roundtrip_error_response<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    error: H::Error,
) {
    write_response(socket, H::error_status(error), H::error_body(error)).await;
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

pub(super) fn validate_upload_auth<H: crate::Host>(header: &str) -> Result<(), UploadAuthError> {
    let expected_token = H::upload_token();
    if crate::token_matches(header.as_bytes(), expected_token) {
        Ok(())
    } else {
        Err(UploadAuthError::MissingOrInvalidToken)
    }
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
            let path = crate::decode_path(encoded.as_bytes()).map_err(|error| match error {
                crate::ParseError::PathTooLong => "path too long",
                crate::ParseError::InvalidPercentEncoding => "bad percent-encoding",
                crate::ParseError::InvalidPath => "path outside upload root",
            })?;
            return Ok((path.bytes, path.len));
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

pub(super) async fn write_response(socket: &mut impl Write, status: &[u8], body: &[u8]) {
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

    let _ = socket.write_all(b"HTTP/1.1 ").await;
    let _ = socket.write_all(status).await;
    let _ = socket
        .write_all(b"\r\nConnection: keep-alive\r\nContent-Length: ")
        .await;
    let _ = socket.write_all(&content_length[idx..]).await;
    let _ = socket.write_all(b"\r\n\r\n").await;
    let _ = socket.write_all(body).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_contract_retains_duplicate_and_malformed_length_errors() {
        assert_eq!(
            parse_content_length("POST / HTTP/1.1\r\nContent-Length: 12\r\n"),
            Ok(Some(12))
        );
        assert_eq!(
            parse_content_length("POST / HTTP/1.1\r\nContent-Length: 12\r\ncontent-length: 12\r\n"),
            Err("duplicate content-length")
        );
        assert_eq!(
            parse_content_length("POST / HTTP/1.1\r\nContent-Length: nope\r\n"),
            Err("invalid content-length")
        );
        assert_eq!(parse_content_length("POST / HTTP/1.1\r\n"), Ok(None));
    }
    #[test]
    fn query_contract_is_bounded_and_root_scoped() {
        assert_eq!(
            parse_path_query("/upload?path=/assets/a%20b&size=4", "/upload")
                .map(|(p, n)| p[..n as usize].to_vec()),
            Ok(b"/assets/a b".to_vec())
        );
        assert_eq!(
            parse_path_query("/upload?path=/assets2/x", "/upload"),
            Err("path outside upload root")
        );
        assert_eq!(
            parse_path_query("/upload?path=/assets/%x0", "/upload"),
            Err("bad percent-encoding")
        );
        assert_eq!(
            parse_u32_query(
                "/upload_begin?path=/assets/x&size=4294967296",
                "/upload_begin",
                "size"
            ),
            Err("invalid query value")
        );
    }
    #[test]
    fn encoded_traversal_remains_visible_to_storage_validation() {
        let (path, len) =
            parse_path_query("/upload?path=/assets/%2e%2e/private", "/upload").unwrap();
        assert_eq!(
            sdcard::upload::validate_path(&path, len as usize, "/assets"),
            Err(sdcard::upload::PathError::InvalidSegment)
        );
        let (path, len) = parse_path_query("/upload?path=/assets/a%00b", "/upload").unwrap();
        assert_eq!(
            sdcard::upload::validate_path(&path, len as usize, "/assets"),
            Err(sdcard::upload::PathError::InvalidSegment)
        );
        let encoded = std::format!("/upload?path=/assets/{}", "x".repeat(SD_PATH_MAX));
        assert_eq!(parse_path_query(&encoded, "/upload"), Err("path too long"));
    }
}
