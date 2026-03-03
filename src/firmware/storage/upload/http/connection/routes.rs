use core::cmp::min;

use embassy_net::tcp::TcpSocket;
use embassy_time::Instant;
use esp_println::println;

use super::super::super::sd_bridge::{roundtrip_error_log, sd_upload_roundtrip};
use super::super::helpers::{
    drain_remaining_body, parse_path_query, parse_u32_query, sd_upload_or_http_error,
    write_response, write_roundtrip_error_response,
};
use super::body::{
    elapsed_ms_u32, forward_upload_body_or_http_error, log_upload_stats, usize_to_u32_saturating,
};
use super::{RequestContext, SdPath};
use crate::firmware::telemetry;
use crate::firmware::types::SdUploadCommand;

pub(super) async fn dispatch_request(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &[u8],
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    match (request.method, request.request_path) {
        ("GET", "/health") => handle_health(socket, request).await,
        ("GET", "/stat") => handle_stat(socket, request).await,
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
    // Contract: /health stays lightweight and independent from SD upload paths.
    // It must remain safe to probe under pressure and must always bump telemetry
    // so host workflows can correlate reachability checks with runtime behavior.
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

async fn handle_stat(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/stat").await?;
    sd_upload_or_http_error(socket, SdUploadCommand::Stat { path, path_len }).await?;
    write_response(socket, b"200 OK", b"stat ok").await;
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
        stats.payload_copy_ms,
        stats.sd_queue_ms,
        stats.sd_task_wait_ms,
        0,
        stats.chunk_p50_ms,
        stats.chunk_p95_ms,
        stats.chunk_max_ms,
        stats.chunk_samples,
        stats.chunk_samples_dropped,
        stats.sd_wait_ms,
        elapsed_ms_u32(request_started_at),
    );
    log_upload_stats("upload_chunk", &stats, stats.sd_wait_ms, request_started_at, 0);

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
    let commit_ms = elapsed_ms_u32(commit_started_at);
    sd_wait_ms = sd_wait_ms.saturating_add(commit_ms);

    let total_sd_wait_ms = sd_wait_ms.saturating_add(stats.sd_wait_ms);
    telemetry::record_upload_http_upload_phase(
        usize_to_u32_saturating(stats.sent_bytes),
        stats.body_read_ms,
        stats.payload_copy_ms,
        stats.sd_queue_ms,
        stats.sd_task_wait_ms,
        commit_ms,
        stats.chunk_p50_ms,
        stats.chunk_p95_ms,
        stats.chunk_max_ms,
        stats.chunk_samples,
        stats.chunk_samples_dropped,
        total_sd_wait_ms,
        elapsed_ms_u32(request_started_at),
    );
    log_upload_stats("upload", &stats, total_sd_wait_ms, request_started_at, commit_ms);

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
