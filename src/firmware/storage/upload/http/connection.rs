use core::cmp::min;

use embassy_net::tcp::TcpSocket;
use embassy_time::{with_timeout, Duration, Instant};

use super::super::super::super::telemetry;
use super::super::super::super::types::SdUploadCommand;
use super::super::sd_bridge::{
    roundtrip_error_log, sd_upload_chunk, sd_upload_roundtrip, SdUploadRoundtripError,
};
use super::helpers::{
    drain_remaining_body, find_header_end, parse_content_length, parse_path_query,
    parse_request_line, parse_u32_query, sd_upload_or_http_error, target_path,
    validate_upload_auth, write_response, write_roundtrip_error_response, UploadAuthError,
};

const HTTP_HEADER_READ_TIMEOUT_MS: u64 = 10_000;

pub(super) async fn handle_connection(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &mut [u8],
) -> Result<(), &'static str> {
    let mut filled = 0usize;
    let header_end = loop {
        if filled == header_buf.len() {
            write_response(socket, b"413 Payload Too Large", b"header too large").await;
            return Err("header too large");
        }

        let n = match with_timeout(
            Duration::from_millis(HTTP_HEADER_READ_TIMEOUT_MS),
            socket.read(&mut header_buf[filled..]),
        )
        .await
        {
            Ok(Ok(n)) => n,
            Ok(Err(_)) => return Err("read"),
            Err(_) => {
                write_response(socket, b"408 Request Timeout", b"request header timeout").await;
                return Err("request header timeout");
            }
        };
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
            telemetry::record_upload_http_health_request();
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
            let request_started_at = Instant::now();
            let prefetched_len = min(body_bytes_in_buffer, content_length);
            let prefetched = &header_buf[body_start..body_start + prefetched_len];
            let stats =
                match forward_upload_body(socket, chunk_buf, prefetched, content_length).await {
                    Ok(stats) => stats,
                    Err(UploadBodyError::ReadBody) => return Err("read body"),
                    Err(UploadBodyError::IncompleteBody) => {
                        write_response(socket, b"400 Bad Request", b"incomplete body").await;
                        return Err("incomplete body");
                    }
                    Err(UploadBodyError::Roundtrip(err)) => {
                        write_roundtrip_error_response(socket, err).await;
                        return Err(roundtrip_error_log(err));
                    }
                };
            telemetry::record_upload_http_upload_phase(
                usize_to_u32_saturating(stats.sent_bytes),
                stats.body_read_ms,
                stats.sd_wait_ms,
                elapsed_ms_u32(request_started_at),
            );

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
            let request_started_at = Instant::now();
            let mut sd_wait_ms = 0u32;
            let expected_size = content_length as u32;
            let begin_started_at = Instant::now();
            sd_upload_or_http_error(
                socket,
                SdUploadCommand::Begin {
                    path,
                    path_len,
                    expected_size,
                },
            )
            .await?;
            sd_wait_ms = sd_wait_ms.saturating_add(elapsed_ms_u32(begin_started_at));

            let prefetched_len = min(body_bytes_in_buffer, content_length);
            let prefetched = &header_buf[body_start..body_start + prefetched_len];
            let stats =
                match forward_upload_body(socket, chunk_buf, prefetched, content_length).await {
                    Ok(stats) => stats,
                    Err(UploadBodyError::ReadBody) => {
                        let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                        return Err("read body");
                    }
                    Err(UploadBodyError::IncompleteBody) => {
                        let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                        write_response(socket, b"400 Bad Request", b"incomplete body").await;
                        return Err("incomplete body");
                    }
                    Err(UploadBodyError::Roundtrip(err)) => {
                        let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                        write_roundtrip_error_response(socket, err).await;
                        return Err(roundtrip_error_log(err));
                    }
                };

            let commit_started_at = Instant::now();
            if let Err(err) = sd_upload_roundtrip(SdUploadCommand::Commit).await {
                let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                write_roundtrip_error_response(socket, err).await;
                return Err(roundtrip_error_log(err));
            }
            sd_wait_ms = sd_wait_ms.saturating_add(elapsed_ms_u32(commit_started_at));
            telemetry::record_upload_http_upload_phase(
                usize_to_u32_saturating(stats.sent_bytes),
                stats.body_read_ms,
                sd_wait_ms.saturating_add(stats.sd_wait_ms),
                elapsed_ms_u32(request_started_at),
            );
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

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

fn usize_to_u32_saturating(value: usize) -> u32 {
    if value > u32::MAX as usize {
        u32::MAX
    } else {
        value as u32
    }
}

struct UploadBodyStats {
    sent_bytes: usize,
    body_read_ms: u32,
    sd_wait_ms: u32,
}

enum UploadBodyError {
    ReadBody,
    IncompleteBody,
    Roundtrip(SdUploadRoundtripError),
}

async fn forward_upload_body(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    prefetched: &[u8],
    content_length: usize,
) -> Result<UploadBodyStats, UploadBodyError> {
    let mut consumed = 0usize;
    let mut pending = 0usize;
    let mut body_read_ms = 0u32;
    let mut sd_wait_ms = 0u32;
    let mut sent_bytes = 0usize;

    let mut prefetched_offset = 0usize;
    while prefetched_offset < prefetched.len() && consumed < content_length {
        let free = chunk_buf.len().saturating_sub(pending);
        let copy_len = min(free, prefetched.len() - prefetched_offset);
        chunk_buf[pending..pending + copy_len]
            .copy_from_slice(&prefetched[prefetched_offset..prefetched_offset + copy_len]);
        pending += copy_len;
        consumed += copy_len;
        prefetched_offset += copy_len;

        if pending == chunk_buf.len() || consumed == content_length {
            let sd_started_at = Instant::now();
            sd_upload_chunk(&chunk_buf[..pending])
                .await
                .map_err(UploadBodyError::Roundtrip)?;
            sd_wait_ms = sd_wait_ms.saturating_add(elapsed_ms_u32(sd_started_at));
            sent_bytes += pending;
            pending = 0;
        }
    }

    while consumed < content_length {
        let want = min(
            chunk_buf.len().saturating_sub(pending),
            content_length - consumed,
        );
        let read_started_at = Instant::now();
        let n = socket
            .read(&mut chunk_buf[pending..pending + want])
            .await
            .map_err(|_| UploadBodyError::ReadBody)?;
        body_read_ms = body_read_ms.saturating_add(elapsed_ms_u32(read_started_at));
        if n == 0 {
            return Err(UploadBodyError::IncompleteBody);
        }
        pending += n;
        consumed += n;

        if pending == chunk_buf.len() || consumed == content_length {
            let sd_started_at = Instant::now();
            sd_upload_chunk(&chunk_buf[..pending])
                .await
                .map_err(UploadBodyError::Roundtrip)?;
            sd_wait_ms = sd_wait_ms.saturating_add(elapsed_ms_u32(sd_started_at));
            sent_bytes += pending;
            pending = 0;
        }
    }

    Ok(UploadBodyStats {
        sent_bytes,
        body_read_ms,
        sd_wait_ms,
    })
}
