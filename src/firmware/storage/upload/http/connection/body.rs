use core::cmp::min;

use embassy_net::tcp::{Error as TcpError, TcpSocket};
use embassy_time::Instant;
use esp_println::println;

use super::super::super::sd_bridge::{
    roundtrip_error_log, sd_upload_chunk, sd_upload_roundtrip, SdUploadRoundtripError,
};
use super::super::helpers::{write_response, write_roundtrip_error_response};
use crate::firmware::telemetry;
use crate::firmware::types::SdUploadCommand;

pub(super) struct UploadBodyStats {
    pub(super) sent_bytes: usize,
    pub(super) chunk_count: u32,
    pub(super) max_chunk_bytes: usize,
    pub(super) body_read_ms: u32,
    pub(super) sd_wait_ms: u32,
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

pub(super) async fn forward_upload_body_or_http_error(
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

pub(super) fn log_upload_stats(
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

pub(super) fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

pub(super) fn usize_to_u32_saturating(value: usize) -> u32 {
    if value > u32::MAX as usize {
        u32::MAX
    } else {
        value as u32
    }
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
