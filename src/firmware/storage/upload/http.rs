use core::cmp::min;

use embassy_net::{tcp::TcpSocket, IpListenEndpoint, Stack};
use embassy_time::{with_timeout, Duration};
use static_cell::StaticCell;

mod helpers;

use helpers::{
    drain_remaining_body, find_header_end, parse_content_length, parse_path_query,
    parse_request_line, parse_u32_query, sd_upload_or_http_error, target_path,
    validate_upload_auth, write_response, write_roundtrip_error_response, UploadAuthError,
};

use super::super::super::types::{SdUploadCommand, SD_UPLOAD_CHUNK_MAX};
use super::sd_bridge::{roundtrip_error_log, sd_upload_chunk, sd_upload_roundtrip};

const UPLOAD_HTTP_PORT: u16 = 8080;
const UPLOAD_HTTP_ROOT: &str = "/assets";
const UPLOAD_HTTP_TOKEN_HEADER: &str = "x-upload-token";
const HTTP_HEADER_MAX: usize = 2048;
const HTTP_RW_BUF: usize = 2048;

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
