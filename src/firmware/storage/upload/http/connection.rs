use core::cmp::min;

use embassy_net::tcp::{Error as TcpError, TcpSocket};
use embassy_time::{with_timeout, Duration, Instant};
use esp_println::println;

use super::super::super::super::telemetry;
use super::super::super::super::types::{SdUploadCommand, SD_PATH_MAX};
use super::super::sd_bridge::{
    roundtrip_error_log, sd_upload_chunk, sd_upload_roundtrip, SdUploadRoundtripError,
};
use super::helpers::{
    drain_remaining_body, find_header_end, parse_content_length, parse_path_query,
    parse_request_line, parse_u32_query, sd_upload_or_http_error, target_path,
    validate_upload_auth, write_response, write_roundtrip_error_response, UploadAuthError,
};

const HTTP_HEADER_READ_TIMEOUT_MS: u64 = 10_000;
type SdPath = ([u8; SD_PATH_MAX], u8);

struct RequestContext<'a> {
    method: &'a str,
    target: &'a str,
    request_path: &'a str,
    content_length: Option<usize>,
    content_length_or_zero: usize,
    body_start: usize,
    body_bytes_in_buffer: usize,
}

pub(super) async fn handle_connection(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &mut [u8],
) -> Result<(), &'static str> {
    let (filled, header_end) = read_header(socket, header_buf).await?;
    let header = core::str::from_utf8(&header_buf[..header_end]).map_err(|_| "header utf8")?;
    let (method, target) = parse_request_line(header).ok_or("bad request line")?;
    let content_length = parse_content_length_or_http_error(socket, header).await?;
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

    authorize_request(socket, header, &request).await?;
    dispatch_request(socket, chunk_buf, header_buf, &request).await
}

async fn read_header(
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

        let n = match with_timeout(
            Duration::from_millis(HTTP_HEADER_READ_TIMEOUT_MS),
            socket.read(&mut header_buf[filled..]),
        )
        .await
        {
            Ok(Ok(n)) => n,
            Ok(Err(err)) => {
                if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
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
                return if matches!(err, TcpError::ConnectionReset) && filled == 0 {
                    Err("read header reset empty")
                } else if matches!(err, TcpError::ConnectionReset) {
                    Err("read header reset")
                } else {
                    Err("read header")
                };
            }
            Err(_) => {
                write_response(socket, b"408 Request Timeout", b"request header timeout").await;
                return Err("request header timeout");
            }
        };

        if n == 0 {
            if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
                println!(
                    "upload_http: header eof filled={} reads={} recv_queue={} state={:?} remote={:?}",
                    filled,
                    header_read_ops,
                    socket.recv_queue(),
                    socket.state(),
                    socket.remote_endpoint(),
                );
            }
            return Err("eof header");
        }

        header_read_ops = header_read_ops.saturating_add(1);
        filled += n;

        if let Some(end) = find_header_end(&header_buf[..filled]) {
            break end;
        }
    };

    Ok((filled, header_end))
}

async fn parse_content_length_or_http_error(
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

async fn authorize_request(
    socket: &mut TcpSocket<'_>,
    header: &str,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
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

async fn dispatch_request(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &[u8],
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    match (request.method, request.request_path) {
        ("GET", "/health") => handle_health(socket, request).await,
        ("POST", "/mkdir") => handle_mkdir(socket, request).await,
        ("DELETE", "/rm") => handle_delete(socket, request).await,
        ("POST", "/upload_begin") => handle_upload_begin(socket, request).await,
        ("PUT", "/upload_chunk") => {
            handle_upload_chunk(socket, chunk_buf, header_buf, request).await
        }
        ("POST", "/upload_commit") => handle_upload_commit(socket, request).await,
        ("POST", "/upload_abort") => handle_upload_abort(socket, request).await,
        ("PUT", "/upload") => handle_upload(socket, chunk_buf, header_buf, request).await,
        _ => handle_not_found(socket, request).await,
    }
}

async fn handle_health(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    telemetry::record_upload_http_health_request();
    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
        println!("upload_http: health ok");
    }
    write_response(socket, b"200 OK", b"ok").await;
    Ok(())
}

async fn handle_mkdir(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/mkdir").await?;
    let path_str = core::str::from_utf8(&path[..path_len as usize]).unwrap_or("<invalid>");
    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_SD) {
        println!("upload_http: mkdir begin path={}", path_str);
    }
    sd_upload_or_http_error(socket, SdUploadCommand::Mkdir { path, path_len }).await?;
    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_SD) {
        println!("upload_http: mkdir done path={}", path_str);
    }
    write_response(socket, b"200 OK", b"mkdir ok").await;
    Ok(())
}

async fn handle_delete(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/rm").await?;
    sd_upload_or_http_error(socket, SdUploadCommand::Remove { path, path_len }).await?;
    write_response(socket, b"200 OK", b"delete ok").await;
    Ok(())
}

async fn handle_upload_begin(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/upload_begin").await?;
    let expected_size = parse_u32_or_400(socket, request.target, "/upload_begin", "size").await?;
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

async fn handle_upload_chunk(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &[u8],
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    let content_length = required_content_length(socket, request.content_length).await?;
    let request_started_at = Instant::now();
    let prefetched = prefetched_body_slice(header_buf, request, content_length);
    let stats =
        forward_upload_body_or_http_error(socket, chunk_buf, prefetched, content_length, false)
            .await?;

    telemetry::record_upload_http_upload_phase(
        usize_to_u32_saturating(stats.sent_bytes),
        stats.body_read_ms,
        stats.sd_wait_ms,
        elapsed_ms_u32(request_started_at),
    );
    log_upload_stats("upload_chunk", &stats, stats.sd_wait_ms, request_started_at);

    write_response(socket, b"200 OK", b"chunk ok").await;
    Ok(())
}

async fn handle_upload_commit(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    sd_upload_or_http_error(socket, SdUploadCommand::Commit).await?;
    write_response(socket, b"200 OK", b"commit ok").await;
    Ok(())
}

async fn handle_upload_abort(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    sd_upload_or_http_error(socket, SdUploadCommand::Abort).await?;
    write_response(socket, b"200 OK", b"abort ok").await;
    Ok(())
}

async fn handle_upload(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &[u8],
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    let content_length = required_content_length(socket, request.content_length).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/upload").await?;
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

    let prefetched = prefetched_body_slice(header_buf, request, content_length);
    let stats =
        forward_upload_body_or_http_error(socket, chunk_buf, prefetched, content_length, true)
            .await?;

    let commit_started_at = Instant::now();
    if let Err(err) = sd_upload_roundtrip(SdUploadCommand::Commit).await {
        let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
        write_roundtrip_error_response(socket, err).await;
        return Err(roundtrip_error_log(err));
    }
    sd_wait_ms = sd_wait_ms.saturating_add(elapsed_ms_u32(commit_started_at));

    let total_sd_wait_ms = sd_wait_ms.saturating_add(stats.sd_wait_ms);
    telemetry::record_upload_http_upload_phase(
        usize_to_u32_saturating(stats.sent_bytes),
        stats.body_read_ms,
        total_sd_wait_ms,
        elapsed_ms_u32(request_started_at),
    );
    log_upload_stats("upload", &stats, total_sd_wait_ms, request_started_at);

    write_response(socket, b"201 Created", b"upload ok").await;
    Ok(())
}

async fn handle_not_found(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    write_response(socket, b"404 Not Found", b"not found").await;
    Ok(())
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

async fn parse_path_or_400(
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

async fn parse_u32_or_400(
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

async fn required_content_length(
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

fn prefetched_body_slice<'a>(
    header_buf: &'a [u8],
    request: &RequestContext<'_>,
    content_length: usize,
) -> &'a [u8] {
    let prefetched_len = min(request.body_bytes_in_buffer, content_length);
    &header_buf[request.body_start..request.body_start + prefetched_len]
}

async fn forward_upload_body_or_http_error(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    prefetched: &[u8],
    content_length: usize,
    abort_on_error: bool,
) -> Result<UploadBodyStats, &'static str> {
    match forward_upload_body(socket, chunk_buf, prefetched, content_length).await {
        Ok(stats) => Ok(stats),
        Err(UploadBodyError::ReadBody {
            err,
            consumed,
            content_length,
            pending,
            want,
        }) => {
            log_upload_body_read_error(socket, err, consumed, content_length, pending, want);
            if abort_on_error {
                abort_upload_roundtrip().await;
            }
            Err("read body")
        }
        Err(UploadBodyError::IncompleteBody) => {
            if abort_on_error {
                abort_upload_roundtrip().await;
            }
            write_response(socket, b"400 Bad Request", b"incomplete body").await;
            Err("incomplete body")
        }
        Err(UploadBodyError::Roundtrip(err)) => {
            if abort_on_error {
                abort_upload_roundtrip().await;
            }
            write_roundtrip_error_response(socket, err).await;
            Err(roundtrip_error_log(err))
        }
    }
}

fn log_upload_body_read_error(
    socket: &TcpSocket<'_>,
    err: TcpError,
    consumed: usize,
    content_length: usize,
    pending: usize,
    want: usize,
) {
    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
        println!(
            "upload_http: body read err={:?} consumed={} of {} pending={} want={} recv_queue={} send_queue={} state={:?} remote={:?}",
            err,
            consumed,
            content_length,
            pending,
            want,
            socket.recv_queue(),
            socket.send_queue(),
            socket.state(),
            socket.remote_endpoint(),
        );
    }
}

async fn abort_upload_roundtrip() {
    let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
}

fn log_upload_stats(
    phase: &str,
    stats: &UploadBodyStats,
    total_sd_wait_ms: u32,
    request_started_at: Instant,
) {
    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
        let avg_chunk = if stats.chunk_count == 0 {
            0
        } else {
            stats.sent_bytes / stats.chunk_count as usize
        };
        println!(
            "upload_http: {} stats bytes={} chunks={} avg_chunk={} max_chunk={} body_ms={} sd_ms={} req_ms={}",
            phase,
            stats.sent_bytes,
            stats.chunk_count,
            avg_chunk,
            stats.max_chunk_bytes,
            stats.body_read_ms,
            total_sd_wait_ms,
            elapsed_ms_u32(request_started_at),
        );
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
    chunk_count: u32,
    max_chunk_bytes: usize,
    body_read_ms: u32,
    sd_wait_ms: u32,
}

enum UploadBodyError {
    ReadBody {
        err: TcpError,
        consumed: usize,
        content_length: usize,
        pending: usize,
        want: usize,
    },
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
    let mut chunk_count = 0u32;
    let mut max_chunk_bytes = 0usize;

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
            chunk_count = chunk_count.saturating_add(1);
            max_chunk_bytes = max_chunk_bytes.max(pending);
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
            .map_err(|err| UploadBodyError::ReadBody {
                err,
                consumed,
                content_length,
                pending,
                want,
            })?;
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
            chunk_count = chunk_count.saturating_add(1);
            max_chunk_bytes = max_chunk_bytes.max(pending);
            pending = 0;
        }
    }

    Ok(UploadBodyStats {
        sent_bytes,
        chunk_count,
        max_chunk_bytes,
        body_read_ms,
        sd_wait_ms,
    })
}
