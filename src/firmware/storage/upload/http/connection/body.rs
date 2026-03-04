use core::cmp::min;

use embassy_futures::yield_now;
use embassy_net::tcp::{Error as TcpError, TcpSocket};
use embassy_time::{with_timeout, Duration, Instant};
use esp_println::println;

use super::super::super::sd_bridge::{
    roundtrip_error_log, sd_upload_chunk_finish, sd_upload_chunk_start, sd_upload_chunk_try_finish,
    sd_upload_roundtrip, SdUploadChunkFinish, SdUploadChunkInFlight, SdUploadChunkTryFinish,
    SdUploadRoundtripError,
};
use super::super::helpers::{write_response, write_roundtrip_error_response};
use crate::firmware::telemetry;
use crate::firmware::types::{
    SdUploadCommand, HTTP_INGRESS_COOP_YIELD_BYTES, HTTP_INGRESS_COOP_YIELD_READS,
};

pub(super) struct UploadBodyStats {
    pub(super) sent_bytes: usize,
    pub(super) chunk_count: u32,
    pub(super) max_chunk_bytes: usize,
    pub(super) body_read_ms: u32,
    pub(super) payload_copy_ms: u32,
    pub(super) sd_queue_ms: u32,
    pub(super) sd_task_wait_ms: u32,
    pub(super) sd_task_queue_wait_ms: u32,
    pub(super) sd_task_handler_ms: u32,
    pub(super) sd_task_residual_ms: u32,
    pub(super) sd_task_post_handler_ms: u32,
    pub(super) sd_task_publish_to_receive_ms: u32,
    pub(super) sd_task_residual_other_ms: u32,
    pub(super) sd_wait_ms: u32,
    pub(super) chunk_p50_ms: u32,
    pub(super) chunk_p95_ms: u32,
    pub(super) chunk_max_ms: u32,
    pub(super) chunk_samples: u32,
    pub(super) chunk_samples_dropped: u32,
    pub(super) ingress_flush_wait_ms: u32,
    pub(super) ingress_read_calls: u32,
    pub(super) ingress_read_pre_queue_bytes_total: u32,
    pub(super) ingress_read_pre_queue_max: u32,
    pub(super) ingress_read_pre_queue_empty_calls: u32,
    pub(super) ingress_read_short_calls: u32,
    pub(super) ingress_read_wait_empty_q_ms: u32,
    pub(super) ingress_read_wait_nonempty_q_ms: u32,
    pub(super) ingress_read_wait_over_10ms: u32,
    pub(super) ingress_read_wait_over_50ms: u32,
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
const INGRESS_READ_WAIT_OVER_10MS: u32 = 10;
const INGRESS_READ_WAIT_OVER_50MS: u32 = 50;

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
        let snapshot = telemetry::snapshot();
        println!(
            "upload_http: {} stats pipeline={} bytes={} chunks={} avg_chunk={} max_chunk={} read_wait_ms={} copy_ms={} sd_queue_ms={} sd_task_ms={} sd_task_queue_wait_ms={} sd_task_handler_ms={} sd_task_residual_ms={} sd_task_post_handler_ms={} sd_task_publish_to_receive_ms={} sd_task_residual_other_ms={} sd_ms={} chunk_p50_ms={} chunk_p95_ms={} chunk_max_ms={} chunk_samples={} chunk_samples_dropped={} ingress_flush_wait_ms={} ingress_read_calls={} ingress_pre_read_q_total={} ingress_pre_read_q_max={} ingress_pre_read_q_empty_calls={} ingress_read_short_calls={} ingress_read_wait_empty_q_ms={} ingress_read_wait_nonempty_q_ms={} ingress_read_wait_over_10ms={} ingress_read_wait_over_50ms={} wifi_rssi_last_dbm={} wifi_rssi_min_dbm={} wifi_rssi_max_dbm={} wifi_rssi_samples={} wifi_rssi_low_samples={} commit_ms={} req_ms={}",
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
            stats.sd_task_queue_wait_ms,
            stats.sd_task_handler_ms,
            stats.sd_task_residual_ms,
            stats.sd_task_post_handler_ms,
            stats.sd_task_publish_to_receive_ms,
            stats.sd_task_residual_other_ms,
            total_sd_wait_ms,
            stats.chunk_p50_ms,
            stats.chunk_p95_ms,
            stats.chunk_max_ms,
            stats.chunk_samples,
            stats.chunk_samples_dropped,
            stats.ingress_flush_wait_ms,
            stats.ingress_read_calls,
            stats.ingress_read_pre_queue_bytes_total,
            stats.ingress_read_pre_queue_max,
            stats.ingress_read_pre_queue_empty_calls,
            stats.ingress_read_short_calls,
            stats.ingress_read_wait_empty_q_ms,
            stats.ingress_read_wait_nonempty_q_ms,
            stats.ingress_read_wait_over_10ms,
            stats.ingress_read_wait_over_50ms,
            snapshot.wifi_link_rssi_last_dbm,
            snapshot.wifi_link_rssi_min_dbm,
            snapshot.wifi_link_rssi_max_dbm,
            snapshot.wifi_link_rssi_samples,
            snapshot.wifi_link_rssi_low_samples,
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
    let mut sd_task_queue_wait_ms = 0u32;
    let mut sd_task_handler_ms = 0u32;
    let mut sd_task_residual_ms = 0u32;
    let mut sd_task_post_handler_ms = 0u32;
    let mut sd_task_publish_to_receive_ms = 0u32;
    let mut sd_task_residual_other_ms = 0u32;
    let mut sd_wait_ms = 0u32;
    let mut sent_bytes = 0usize;
    let mut chunk_count = 0u32;
    let mut max_chunk_bytes = 0usize;
    let mut ingress_flush_wait_ms = 0u32;
    let mut ingress_read_calls = 0u32;
    let mut ingress_read_pre_queue_bytes_total = 0u32;
    let mut ingress_read_pre_queue_max = 0u32;
    let mut ingress_read_pre_queue_empty_calls = 0u32;
    let mut ingress_read_short_calls = 0u32;
    let mut ingress_read_wait_empty_q_ms = 0u32;
    let mut ingress_read_wait_nonempty_q_ms = 0u32;
    let mut ingress_read_wait_over_10ms = 0u32;
    let mut ingress_read_wait_over_50ms = 0u32;
    let mut ingress_read_bytes_since_yield = 0usize;
    let mut ingress_read_ops_since_yield = 0u32;
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
                &mut sd_task_queue_wait_ms,
                &mut sd_task_handler_ms,
                &mut sd_task_residual_ms,
                &mut sd_task_post_handler_ms,
                &mut sd_task_publish_to_receive_ms,
                &mut sd_task_residual_other_ms,
                &mut sd_wait_ms,
                &mut sent_bytes,
                &mut chunk_count,
                &mut max_chunk_bytes,
                &mut chunk_samples,
                &mut ingress_flush_wait_ms,
            )
            .await?;
            pending = 0;
        }
    }

    while consumed < content_length {
        if UPLOAD_CHUNK_PIPELINE_ENABLED {
            try_drain_inflight_chunk(
                &mut inflight,
                &mut payload_copy_ms,
                &mut sd_queue_ms,
                &mut sd_task_wait_ms,
                &mut sd_task_queue_wait_ms,
                &mut sd_task_handler_ms,
                &mut sd_task_residual_ms,
                &mut sd_task_post_handler_ms,
                &mut sd_task_publish_to_receive_ms,
                &mut sd_task_residual_other_ms,
                &mut sd_wait_ms,
                &mut sent_bytes,
                &mut chunk_count,
                &mut max_chunk_bytes,
                &mut chunk_samples,
            )?;
        }
        let want = min(
            chunk_buf.len().saturating_sub(pending),
            content_length - consumed,
        );
        let pre_read_queue = usize_to_u32_saturating(socket.recv_queue());
        ingress_read_calls = ingress_read_calls.saturating_add(1);
        ingress_read_pre_queue_bytes_total =
            ingress_read_pre_queue_bytes_total.saturating_add(pre_read_queue);
        ingress_read_pre_queue_max = ingress_read_pre_queue_max.max(pre_read_queue);
        if pre_read_queue == 0 {
            ingress_read_pre_queue_empty_calls =
                ingress_read_pre_queue_empty_calls.saturating_add(1);
        }
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
        let read_wait_ms = elapsed_ms_u32(read_started_at);
        body_read_ms = body_read_ms.saturating_add(read_wait_ms);
        if pre_read_queue == 0 {
            ingress_read_wait_empty_q_ms =
                ingress_read_wait_empty_q_ms.saturating_add(read_wait_ms);
        } else {
            ingress_read_wait_nonempty_q_ms =
                ingress_read_wait_nonempty_q_ms.saturating_add(read_wait_ms);
        }
        if read_wait_ms >= INGRESS_READ_WAIT_OVER_10MS {
            ingress_read_wait_over_10ms = ingress_read_wait_over_10ms.saturating_add(1);
        }
        if read_wait_ms >= INGRESS_READ_WAIT_OVER_50MS {
            ingress_read_wait_over_50ms = ingress_read_wait_over_50ms.saturating_add(1);
        }
        if n == 0 {
            drain_inflight_on_error(&mut inflight).await;
            return Err(UploadBodyError::IncompleteBody);
        }
        if n < want {
            ingress_read_short_calls = ingress_read_short_calls.saturating_add(1);
        }
        pending += n;
        consumed += n;
        if pre_read_queue > 0 {
            ingress_read_bytes_since_yield = ingress_read_bytes_since_yield.saturating_add(n);
            ingress_read_ops_since_yield = ingress_read_ops_since_yield.saturating_add(1);
        } else {
            ingress_read_bytes_since_yield = 0;
            ingress_read_ops_since_yield = 0;
        }

        if pending == chunk_buf.len() || consumed == content_length {
            queue_chunk_for_sd(
                &chunk_buf[..pending],
                &mut inflight,
                &mut payload_copy_ms,
                &mut sd_queue_ms,
                &mut sd_task_wait_ms,
                &mut sd_task_queue_wait_ms,
                &mut sd_task_handler_ms,
                &mut sd_task_residual_ms,
                &mut sd_task_post_handler_ms,
                &mut sd_task_publish_to_receive_ms,
                &mut sd_task_residual_other_ms,
                &mut sd_wait_ms,
                &mut sent_bytes,
                &mut chunk_count,
                &mut max_chunk_bytes,
                &mut chunk_samples,
                &mut ingress_flush_wait_ms,
            )
            .await?;
            pending = 0;
        }

        if ingress_read_bytes_since_yield >= HTTP_INGRESS_COOP_YIELD_BYTES
            || ingress_read_ops_since_yield >= HTTP_INGRESS_COOP_YIELD_READS
        {
            // Keep the net runner from being starved by long, immediately-ready
            // read bursts in this cooperative executor.
            yield_now().await;
            ingress_read_bytes_since_yield = 0;
            ingress_read_ops_since_yield = 0;
        }
    }
    flush_inflight_chunk(
        &mut inflight,
        &mut payload_copy_ms,
        &mut sd_queue_ms,
        &mut sd_task_wait_ms,
        &mut sd_task_queue_wait_ms,
        &mut sd_task_handler_ms,
        &mut sd_task_residual_ms,
        &mut sd_task_post_handler_ms,
        &mut sd_task_publish_to_receive_ms,
        &mut sd_task_residual_other_ms,
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
        sd_task_queue_wait_ms,
        sd_task_handler_ms,
        sd_task_residual_ms,
        sd_task_post_handler_ms,
        sd_task_publish_to_receive_ms,
        sd_task_residual_other_ms,
        sd_wait_ms,
        chunk_p50_ms,
        chunk_p95_ms,
        chunk_max_ms: chunk_samples.max_ms,
        chunk_samples: chunk_samples.len as u32,
        chunk_samples_dropped: chunk_samples.dropped,
        ingress_flush_wait_ms,
        ingress_read_calls,
        ingress_read_pre_queue_bytes_total,
        ingress_read_pre_queue_max,
        ingress_read_pre_queue_empty_calls,
        ingress_read_short_calls,
        ingress_read_wait_empty_q_ms,
        ingress_read_wait_nonempty_q_ms,
        ingress_read_wait_over_10ms,
        ingress_read_wait_over_50ms,
    })
}

async fn queue_chunk_for_sd(
    data: &[u8],
    inflight: &mut Option<InflightChunk>,
    payload_copy_ms: &mut u32,
    sd_queue_ms: &mut u32,
    sd_task_wait_ms: &mut u32,
    sd_task_queue_wait_ms: &mut u32,
    sd_task_handler_ms: &mut u32,
    sd_task_residual_ms: &mut u32,
    sd_task_post_handler_ms: &mut u32,
    sd_task_publish_to_receive_ms: &mut u32,
    sd_task_residual_other_ms: &mut u32,
    sd_wait_ms: &mut u32,
    sent_bytes: &mut usize,
    chunk_count: &mut u32,
    max_chunk_bytes: &mut usize,
    chunk_samples: &mut ChunkLatencySamples,
    ingress_flush_wait_ms: &mut u32,
) -> Result<(), UploadBodyError> {
    let ingress_flush_started_at = Instant::now();
    flush_inflight_chunk(
        inflight,
        payload_copy_ms,
        sd_queue_ms,
        sd_task_wait_ms,
        sd_task_queue_wait_ms,
        sd_task_handler_ms,
        sd_task_residual_ms,
        sd_task_post_handler_ms,
        sd_task_publish_to_receive_ms,
        sd_task_residual_other_ms,
        sd_wait_ms,
        sent_bytes,
        chunk_count,
        max_chunk_bytes,
        chunk_samples,
    )
    .await?;
    *ingress_flush_wait_ms =
        ingress_flush_wait_ms.saturating_add(elapsed_ms_u32(ingress_flush_started_at));
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
            sd_task_queue_wait_ms,
            sd_task_handler_ms,
            sd_task_residual_ms,
            sd_task_post_handler_ms,
            sd_task_publish_to_receive_ms,
            sd_task_residual_other_ms,
            sd_wait_ms,
            sent_bytes,
            chunk_count,
            max_chunk_bytes,
            chunk_samples,
        )
        .await?;
        *ingress_flush_wait_ms =
            ingress_flush_wait_ms.saturating_add(elapsed_ms_u32(queue_started_at));
    }
    Ok(())
}

async fn flush_inflight_chunk(
    inflight: &mut Option<InflightChunk>,
    payload_copy_ms: &mut u32,
    sd_queue_ms: &mut u32,
    sd_task_wait_ms: &mut u32,
    sd_task_queue_wait_ms: &mut u32,
    sd_task_handler_ms: &mut u32,
    sd_task_residual_ms: &mut u32,
    sd_task_post_handler_ms: &mut u32,
    sd_task_publish_to_receive_ms: &mut u32,
    sd_task_residual_other_ms: &mut u32,
    sd_wait_ms: &mut u32,
    sent_bytes: &mut usize,
    chunk_count: &mut u32,
    max_chunk_bytes: &mut usize,
    chunk_samples: &mut ChunkLatencySamples,
) -> Result<(), UploadBodyError> {
    let Some(inflight_chunk) = inflight.take() else {
        return Ok(());
    };
    let InflightChunk {
        transfer,
        len,
        queue_ms,
        copy_ms,
    } = inflight_chunk;
    let chunk_finish = sd_upload_chunk_finish(transfer)
        .await
        .map_err(UploadBodyError::Roundtrip)?;
    apply_finished_chunk(
        len,
        queue_ms,
        copy_ms,
        chunk_finish,
        payload_copy_ms,
        sd_queue_ms,
        sd_task_wait_ms,
        sd_task_queue_wait_ms,
        sd_task_handler_ms,
        sd_task_residual_ms,
        sd_task_post_handler_ms,
        sd_task_publish_to_receive_ms,
        sd_task_residual_other_ms,
        sd_wait_ms,
        sent_bytes,
        chunk_count,
        max_chunk_bytes,
        chunk_samples,
    );
    Ok(())
}

fn try_drain_inflight_chunk(
    inflight: &mut Option<InflightChunk>,
    payload_copy_ms: &mut u32,
    sd_queue_ms: &mut u32,
    sd_task_wait_ms: &mut u32,
    sd_task_queue_wait_ms: &mut u32,
    sd_task_handler_ms: &mut u32,
    sd_task_residual_ms: &mut u32,
    sd_task_post_handler_ms: &mut u32,
    sd_task_publish_to_receive_ms: &mut u32,
    sd_task_residual_other_ms: &mut u32,
    sd_wait_ms: &mut u32,
    sent_bytes: &mut usize,
    chunk_count: &mut u32,
    max_chunk_bytes: &mut usize,
    chunk_samples: &mut ChunkLatencySamples,
) -> Result<(), UploadBodyError> {
    let Some(inflight_chunk) = inflight.take() else {
        return Ok(());
    };
    let InflightChunk {
        transfer,
        len,
        queue_ms,
        copy_ms,
    } = inflight_chunk;
    match sd_upload_chunk_try_finish(transfer).map_err(UploadBodyError::Roundtrip)? {
        SdUploadChunkTryFinish::Pending(transfer) => {
            *inflight = Some(InflightChunk {
                transfer,
                len,
                queue_ms,
                copy_ms,
            });
        }
        SdUploadChunkTryFinish::Finished(chunk_finish) => {
            apply_finished_chunk(
                len,
                queue_ms,
                copy_ms,
                chunk_finish,
                payload_copy_ms,
                sd_queue_ms,
                sd_task_wait_ms,
                sd_task_queue_wait_ms,
                sd_task_handler_ms,
                sd_task_residual_ms,
                sd_task_post_handler_ms,
                sd_task_publish_to_receive_ms,
                sd_task_residual_other_ms,
                sd_wait_ms,
                sent_bytes,
                chunk_count,
                max_chunk_bytes,
                chunk_samples,
            );
        }
    }
    Ok(())
}

fn apply_finished_chunk(
    len: usize,
    queue_ms: u32,
    copy_ms: u32,
    chunk_finish: SdUploadChunkFinish,
    payload_copy_ms: &mut u32,
    sd_queue_ms: &mut u32,
    sd_task_wait_ms: &mut u32,
    sd_task_queue_wait_ms: &mut u32,
    sd_task_handler_ms: &mut u32,
    sd_task_residual_ms: &mut u32,
    sd_task_post_handler_ms: &mut u32,
    sd_task_publish_to_receive_ms: &mut u32,
    sd_task_residual_other_ms: &mut u32,
    sd_wait_ms: &mut u32,
    sent_bytes: &mut usize,
    chunk_count: &mut u32,
    max_chunk_bytes: &mut usize,
    chunk_samples: &mut ChunkLatencySamples,
) {
    let roundtrip_ms = chunk_finish.roundtrip_ms;
    let task_wait_ms = roundtrip_ms.saturating_sub(queue_ms);
    let task_residual_ms = task_wait_ms
        .saturating_sub(chunk_finish.queue_wait_ms)
        .saturating_sub(chunk_finish.handler_ms);
    let post_handler_ms = chunk_finish.post_handler_ms.min(task_residual_ms);
    let residual_after_post_handler = task_residual_ms.saturating_sub(post_handler_ms);
    let publish_to_receive_ms = chunk_finish
        .publish_to_receive_ms
        .min(residual_after_post_handler);
    let residual_other_ms = residual_after_post_handler.saturating_sub(publish_to_receive_ms);
    *payload_copy_ms = payload_copy_ms.saturating_add(copy_ms);
    *sd_queue_ms = sd_queue_ms.saturating_add(queue_ms);
    *sd_task_wait_ms = sd_task_wait_ms.saturating_add(task_wait_ms);
    *sd_task_queue_wait_ms = sd_task_queue_wait_ms.saturating_add(chunk_finish.queue_wait_ms);
    *sd_task_handler_ms = sd_task_handler_ms.saturating_add(chunk_finish.handler_ms);
    *sd_task_residual_ms = sd_task_residual_ms.saturating_add(task_residual_ms);
    *sd_task_post_handler_ms = sd_task_post_handler_ms.saturating_add(post_handler_ms);
    *sd_task_publish_to_receive_ms =
        sd_task_publish_to_receive_ms.saturating_add(publish_to_receive_ms);
    *sd_task_residual_other_ms = sd_task_residual_other_ms.saturating_add(residual_other_ms);
    *sd_wait_ms = sd_wait_ms.saturating_add(roundtrip_ms);
    *sent_bytes = sent_bytes.saturating_add(len);
    *chunk_count = chunk_count.saturating_add(1);
    *max_chunk_bytes = (*max_chunk_bytes).max(len);
    record_chunk_latency_sample(chunk_samples, roundtrip_ms);
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
