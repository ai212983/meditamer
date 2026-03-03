use core::cmp::min;

use embassy_net::tcp::{Error as TcpError, TcpSocket};
use embassy_time::{with_timeout, Duration, Instant};
use esp_println::println;

use super::super::super::sd_bridge::{
    roundtrip_error_log, sd_upload_chunk_finish, sd_upload_chunk_start, sd_upload_roundtrip,
    SdUploadChunkInFlight, SdUploadRoundtripError,
};
use super::super::helpers::{write_response, write_roundtrip_error_response};
use crate::firmware::telemetry;
use crate::firmware::types::SdUploadCommand;

pub(super) struct UploadBodyStats {
    pub(super) sent_bytes: usize,
    pub(super) chunk_count: u32,
    pub(super) max_chunk_bytes: usize,
    pub(super) body_read_ms: u32,
    pub(super) payload_copy_ms: u32,
    pub(super) sd_queue_ms: u32,
    pub(super) sd_task_wait_ms: u32,
    pub(super) sd_wait_ms: u32,
    pub(super) chunk_p50_ms: u32,
    pub(super) chunk_p95_ms: u32,
    pub(super) chunk_max_ms: u32,
    pub(super) chunk_samples: u32,
    pub(super) chunk_samples_dropped: u32,
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

struct InflightChunk {
    transfer: SdUploadChunkInFlight,
    len: usize,
    queue_ms: u32,
    copy_ms: u32,
}

const CHUNK_LATENCY_SAMPLE_CAP: usize = 32;
const UPLOAD_CHUNK_PIPELINE_ENABLED: bool = cfg!(feature = "asset-upload-http-pipeline");
const UPLOAD_ABORT_RECOVERY_TIMEOUT_MS: u64 = 1_500;

struct ChunkLatencySamples {
    values: [u16; CHUNK_LATENCY_SAMPLE_CAP],
    len: usize,
    dropped: u32,
    max_ms: u32,
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
            let reset = matches!(err, TcpError::ConnectionReset);
            if reset {
                telemetry::record_upload_http_read_body_reset();
            }
            log_upload_body_read_error(socket, err, consumed, content_length, pending, want);
            if abort_on_error {
                abort_upload_roundtrip_bounded(if reset {
                    "read_body_reset"
                } else {
                    "read_body"
                })
                .await;
            }
            Err("read body")
        }
        Err(UploadBodyError::IncompleteBody) => {
            if abort_on_error {
                abort_upload_roundtrip_bounded("incomplete_body").await;
            }
            write_response(socket, b"400 Bad Request", b"incomplete body").await;
            Err("incomplete body")
        }
        Err(UploadBodyError::Roundtrip(err)) => {
            if abort_on_error {
                abort_upload_roundtrip_bounded("roundtrip_err").await;
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
    commit_ms: u32,
) {
    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
        let avg_chunk = if stats.chunk_count == 0 {
            0
        } else {
            stats.sent_bytes / stats.chunk_count as usize
        };
        println!(
            "upload_http: {} stats pipeline={} bytes={} chunks={} avg_chunk={} max_chunk={} read_wait_ms={} copy_ms={} sd_queue_ms={} sd_task_ms={} sd_ms={} chunk_p50_ms={} chunk_p95_ms={} chunk_max_ms={} chunk_samples={} chunk_samples_dropped={} commit_ms={} req_ms={}",
            phase,
            if UPLOAD_CHUNK_PIPELINE_ENABLED {
                "on"
            } else {
                "off"
            },
            stats.sent_bytes,
            stats.chunk_count,
            avg_chunk,
            stats.max_chunk_bytes,
            stats.body_read_ms,
            stats.payload_copy_ms,
            stats.sd_queue_ms,
            stats.sd_task_wait_ms,
            total_sd_wait_ms,
            stats.chunk_p50_ms,
            stats.chunk_p95_ms,
            stats.chunk_max_ms,
            stats.chunk_samples,
            stats.chunk_samples_dropped,
            commit_ms,
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
    let mut payload_copy_ms = 0u32;
    let mut sd_queue_ms = 0u32;
    let mut sd_task_wait_ms = 0u32;
    let mut sd_wait_ms = 0u32;
    let mut sent_bytes = 0usize;
    let mut chunk_count = 0u32;
    let mut max_chunk_bytes = 0usize;
    let mut inflight: Option<InflightChunk> = None;
    let mut chunk_samples = ChunkLatencySamples {
        values: [0; CHUNK_LATENCY_SAMPLE_CAP],
        len: 0,
        dropped: 0,
        max_ms: 0,
    };

    let mut prefetched_offset = 0usize;
    while prefetched_offset < prefetched.len() && consumed < content_length {
        let free = chunk_buf.len().saturating_sub(pending);
        let copy_len = min(free, prefetched.len() - prefetched_offset);
        let copy_started_at = Instant::now();
        chunk_buf[pending..pending + copy_len]
            .copy_from_slice(&prefetched[prefetched_offset..prefetched_offset + copy_len]);
        payload_copy_ms = payload_copy_ms.saturating_add(elapsed_ms_u32(copy_started_at));
        pending += copy_len;
        consumed += copy_len;
        prefetched_offset += copy_len;

        if pending == chunk_buf.len() || consumed == content_length {
            queue_chunk_for_sd(
                &chunk_buf[..pending],
                &mut inflight,
                &mut payload_copy_ms,
                &mut sd_queue_ms,
                &mut sd_task_wait_ms,
                &mut sd_wait_ms,
                &mut sent_bytes,
                &mut chunk_count,
                &mut max_chunk_bytes,
                &mut chunk_samples,
            )
            .await?;
            pending = 0;
        }
    }

    while consumed < content_length {
        let want = min(
            chunk_buf.len().saturating_sub(pending),
            content_length - consumed,
        );
        let read_started_at = Instant::now();
        let n = match socket.read(&mut chunk_buf[pending..pending + want]).await {
            Ok(n) => n,
            Err(err) => {
                drain_inflight_on_error(&mut inflight).await;
                return Err(UploadBodyError::ReadBody {
                    err,
                    consumed,
                    content_length,
                    pending,
                    want,
                });
            }
        };
        body_read_ms = body_read_ms.saturating_add(elapsed_ms_u32(read_started_at));
        if n == 0 {
            drain_inflight_on_error(&mut inflight).await;
            return Err(UploadBodyError::IncompleteBody);
        }
        pending += n;
        consumed += n;

        if pending == chunk_buf.len() || consumed == content_length {
            queue_chunk_for_sd(
                &chunk_buf[..pending],
                &mut inflight,
                &mut payload_copy_ms,
                &mut sd_queue_ms,
                &mut sd_task_wait_ms,
                &mut sd_wait_ms,
                &mut sent_bytes,
                &mut chunk_count,
                &mut max_chunk_bytes,
                &mut chunk_samples,
            )
            .await?;
            pending = 0;
        }
    }
    flush_inflight_chunk(
        &mut inflight,
        &mut payload_copy_ms,
        &mut sd_queue_ms,
        &mut sd_task_wait_ms,
        &mut sd_wait_ms,
        &mut sent_bytes,
        &mut chunk_count,
        &mut max_chunk_bytes,
        &mut chunk_samples,
    )
    .await?;
    let (chunk_p50_ms, chunk_p95_ms) = chunk_latency_quantiles(&chunk_samples);

    Ok(UploadBodyStats {
        sent_bytes,
        chunk_count,
        max_chunk_bytes,
        body_read_ms,
        payload_copy_ms,
        sd_queue_ms,
        sd_task_wait_ms,
        sd_wait_ms,
        chunk_p50_ms,
        chunk_p95_ms,
        chunk_max_ms: chunk_samples.max_ms,
        chunk_samples: chunk_samples.len as u32,
        chunk_samples_dropped: chunk_samples.dropped,
    })
}

async fn queue_chunk_for_sd(
    data: &[u8],
    inflight: &mut Option<InflightChunk>,
    payload_copy_ms: &mut u32,
    sd_queue_ms: &mut u32,
    sd_task_wait_ms: &mut u32,
    sd_wait_ms: &mut u32,
    sent_bytes: &mut usize,
    chunk_count: &mut u32,
    max_chunk_bytes: &mut usize,
    chunk_samples: &mut ChunkLatencySamples,
) -> Result<(), UploadBodyError> {
    flush_inflight_chunk(
        inflight,
        payload_copy_ms,
        sd_queue_ms,
        sd_task_wait_ms,
        sd_wait_ms,
        sent_bytes,
        chunk_count,
        max_chunk_bytes,
        chunk_samples,
    )
    .await?;
    let queue_started_at = Instant::now();
    let transfer = sd_upload_chunk_start(data)
        .await
        .map_err(UploadBodyError::Roundtrip)?;
    let queue_ms = elapsed_ms_u32(queue_started_at);
    *inflight = Some(InflightChunk {
        copy_ms: transfer.copy_ms,
        queue_ms,
        len: data.len(),
        transfer,
    });
    if !UPLOAD_CHUNK_PIPELINE_ENABLED {
        flush_inflight_chunk(
            inflight,
            payload_copy_ms,
            sd_queue_ms,
            sd_task_wait_ms,
            sd_wait_ms,
            sent_bytes,
            chunk_count,
            max_chunk_bytes,
            chunk_samples,
        )
        .await?;
    }
    Ok(())
}

async fn flush_inflight_chunk(
    inflight: &mut Option<InflightChunk>,
    payload_copy_ms: &mut u32,
    sd_queue_ms: &mut u32,
    sd_task_wait_ms: &mut u32,
    sd_wait_ms: &mut u32,
    sent_bytes: &mut usize,
    chunk_count: &mut u32,
    max_chunk_bytes: &mut usize,
    chunk_samples: &mut ChunkLatencySamples,
) -> Result<(), UploadBodyError> {
    let Some(inflight_chunk) = inflight.take() else {
        return Ok(());
    };
    let roundtrip_ms = sd_upload_chunk_finish(inflight_chunk.transfer)
        .await
        .map_err(UploadBodyError::Roundtrip)?;
    let task_wait_ms = roundtrip_ms.saturating_sub(inflight_chunk.queue_ms);
    *payload_copy_ms = payload_copy_ms.saturating_add(inflight_chunk.copy_ms);
    *sd_queue_ms = sd_queue_ms.saturating_add(inflight_chunk.queue_ms);
    *sd_task_wait_ms = sd_task_wait_ms.saturating_add(task_wait_ms);
    *sd_wait_ms = sd_wait_ms.saturating_add(roundtrip_ms);
    *sent_bytes = sent_bytes.saturating_add(inflight_chunk.len);
    *chunk_count = chunk_count.saturating_add(1);
    *max_chunk_bytes = (*max_chunk_bytes).max(inflight_chunk.len);
    record_chunk_latency_sample(chunk_samples, roundtrip_ms);
    Ok(())
}

async fn drain_inflight_on_error(inflight: &mut Option<InflightChunk>) {
    let Some(inflight_chunk) = inflight.take() else {
        return;
    };
    let _ = sd_upload_chunk_finish(inflight_chunk.transfer).await;
}

fn record_chunk_latency_sample(samples: &mut ChunkLatencySamples, latency_ms: u32) {
    samples.max_ms = samples.max_ms.max(latency_ms);
    let latency_u16 = latency_ms.min(u16::MAX as u32) as u16;
    if samples.len < CHUNK_LATENCY_SAMPLE_CAP {
        samples.values[samples.len] = latency_u16;
        samples.len += 1;
    } else {
        samples.dropped = samples.dropped.saturating_add(1);
    }
}

fn chunk_latency_quantiles(samples: &ChunkLatencySamples) -> (u32, u32) {
    if samples.len == 0 {
        return (0, 0);
    }
    let mut sorted = [0u16; CHUNK_LATENCY_SAMPLE_CAP];
    sorted[..samples.len].copy_from_slice(&samples.values[..samples.len]);
    sorted[..samples.len].sort_unstable();

    let p50_idx = ((samples.len - 1) * 50) / 100;
    let p95_idx = ((samples.len - 1) * 95) / 100;
    (sorted[p50_idx] as u32, sorted[p95_idx] as u32)
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

async fn abort_upload_roundtrip_bounded(reason: &str) {
    let abort_result = with_timeout(
        Duration::from_millis(UPLOAD_ABORT_RECOVERY_TIMEOUT_MS),
        sd_upload_roundtrip(SdUploadCommand::Abort),
    )
    .await;
    if let Ok(Err(err)) = abort_result {
        if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
            println!(
                "upload_http: abort recovery err={} reason={}",
                roundtrip_error_log(err),
                reason
            );
        }
    }
    if abort_result.is_err() && telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
        println!(
            "upload_http: abort recovery timeout reason={} timeout_ms={}",
            reason, UPLOAD_ABORT_RECOVERY_TIMEOUT_MS
        );
    }
}
