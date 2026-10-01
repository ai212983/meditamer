use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Instant};

use super::super::super::HTTP_SOCKET_TIMEOUT_SECS;
use super::super::body::{
    elapsed_ms_u32, forward_upload_body_or_http_error, log_upload_stats, usize_to_u32_saturating,
};
use super::super::{RequestContext, HTTP_UPLOAD_BODY_READ_TIMEOUT_MS};
use super::params::{
    parse_path_or_400, parse_u32_or_400, prefetched_body_slice, required_content_length,
    sd_upload_or_http_error, write_response, write_roundtrip_error_response,
};

pub(super) async fn handle_upload_begin<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    H::log_stack_headroom("http_upload_begin_route_entry");
    super::params::drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/upload_begin").await?;
    let expected_size = parse_u32_or_400(socket, request.target, "/upload_begin", "size").await?;
    sd_upload_or_http_error::<H>(
        socket,
        crate::Command::Begin {
            path,
            path_len,
            expected_size,
        },
    )
    .await?;
    super::write_success_response(socket, super::RequestRouteKind::UploadBegin).await;
    Ok(())
}

pub(super) async fn handle_upload_chunk<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &[u8],
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    let content_length = required_content_length(socket, request.content_length).await?;
    let request_started_at = Instant::now();
    let prefetched = prefetched_body_slice(header_buf, request, content_length);
    socket.set_timeout(Some(Duration::from_millis(
        HTTP_UPLOAD_BODY_READ_TIMEOUT_MS,
    )));
    let body_result = forward_upload_body_or_http_error::<H>(
        socket,
        chunk_buf,
        prefetched,
        content_length,
        false,
    )
    .await;
    socket.set_timeout(Some(Duration::from_secs(HTTP_SOCKET_TIMEOUT_SECS)));
    let stats = body_result?;

    H::record_upload_http_upload_phase(crate::UploadHttpPhaseMetrics {
        bytes: usize_to_u32_saturating(stats.sent_bytes),
        body_read_ms: stats.body_read_ms,
        payload_copy_ms: stats.payload_copy_ms,
        sd_queue_ms: stats.sd_queue_ms,
        sd_task_wait_ms: stats.sd_task_wait_ms,
        commit_ms: 0,
        chunk_p50_ms: stats.chunk_p50_ms,
        chunk_p95_ms: stats.chunk_p95_ms,
        chunk_max_ms: stats.chunk_max_ms,
        chunk_samples: stats.chunk_samples,
        chunk_samples_dropped: stats.chunk_samples_dropped,
        sd_wait_ms: stats.sd_wait_ms,
        request_ms: elapsed_ms_u32(request_started_at),
    });
    log_upload_stats::<H>(
        "upload_chunk",
        &stats,
        stats.sd_wait_ms,
        request_started_at,
        0,
    );

    super::write_success_response(socket, super::RequestRouteKind::UploadChunk).await;
    Ok(())
}

pub(super) async fn handle_upload_commit<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    super::params::drain_body(socket, request).await?;
    sd_upload_or_http_error::<H>(socket, crate::Command::Commit).await?;
    super::write_success_response(socket, super::RequestRouteKind::UploadCommit).await;
    Ok(())
}

pub(super) async fn handle_upload_abort<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    super::params::drain_body(socket, request).await?;
    sd_upload_or_http_error::<H>(socket, crate::Command::Abort).await?;
    super::write_success_response(socket, super::RequestRouteKind::UploadAbort).await;
    Ok(())
}

pub(super) async fn handle_upload<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &[u8],
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    H::log_stack_headroom("http_upload_route_entry");
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
    H::log_stack_headroom("http_upload_sd_begin_before");
    sd_upload_or_http_error::<H>(
        socket,
        crate::Command::Begin {
            path,
            path_len,
            expected_size,
        },
    )
    .await?;
    H::log_stack_headroom("http_upload_sd_begin_after");
    sd_wait_ms = sd_wait_ms.saturating_add(elapsed_ms_u32(begin_started_at));

    let prefetched = prefetched_body_slice(header_buf, request, content_length);
    socket.set_timeout(Some(Duration::from_millis(
        HTTP_UPLOAD_BODY_READ_TIMEOUT_MS,
    )));
    let body_result =
        forward_upload_body_or_http_error::<H>(socket, chunk_buf, prefetched, content_length, true)
            .await;
    socket.set_timeout(Some(Duration::from_secs(HTTP_SOCKET_TIMEOUT_SECS)));
    let stats = body_result?;

    let commit_started_at = Instant::now();
    if let Err(err) = H::roundtrip(crate::Command::Commit).await {
        let _ = H::roundtrip(crate::Command::Abort).await;
        write_roundtrip_error_response::<H>(socket, err).await;
        return Err(H::error_log(err));
    }
    let commit_ms = elapsed_ms_u32(commit_started_at);
    sd_wait_ms = sd_wait_ms.saturating_add(commit_ms);

    let total_sd_wait_ms = sd_wait_ms.saturating_add(stats.sd_wait_ms);
    H::record_upload_http_upload_phase(crate::UploadHttpPhaseMetrics {
        bytes: usize_to_u32_saturating(stats.sent_bytes),
        body_read_ms: stats.body_read_ms,
        payload_copy_ms: stats.payload_copy_ms,
        sd_queue_ms: stats.sd_queue_ms,
        sd_task_wait_ms: stats.sd_task_wait_ms,
        commit_ms,
        chunk_p50_ms: stats.chunk_p50_ms,
        chunk_p95_ms: stats.chunk_p95_ms,
        chunk_max_ms: stats.chunk_max_ms,
        chunk_samples: stats.chunk_samples,
        chunk_samples_dropped: stats.chunk_samples_dropped,
        sd_wait_ms: total_sd_wait_ms,
        request_ms: elapsed_ms_u32(request_started_at),
    });
    log_upload_stats::<H>(
        "upload",
        &stats,
        total_sd_wait_ms,
        request_started_at,
        commit_ms,
    );

    super::write_success_response(socket, super::RequestRouteKind::Upload).await;
    Ok(())
}
