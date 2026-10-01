use core::cmp::min;

use embassy_futures::yield_now;
use embassy_net::tcp::{Error as TcpError, TcpSocket};

use super::super::helpers::{write_response, write_roundtrip_error_response};
use super::fairness::IngressFairnessAdaptive;

mod error;
mod latency;
mod pipeline;
mod progress;
mod stats;

use error::{abort_upload_roundtrip_bounded, log_upload_body_read_error, UploadBodyError};
use pipeline::{
    drain_inflight_on_error, flush_inflight_chunk, queue_chunk_for_sd, try_drain_inflight_chunk,
    InflightChunk,
};
use progress::UploadBodyProgress;
pub(crate) use stats::{
    elapsed_ms_u32, log_upload_stats, usize_to_u32_saturating, UploadBodyStats,
};

pub(super) async fn forward_upload_body_or_http_error<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    prefetched: &[u8],
    content_length: usize,
    abort_on_error: bool,
) -> Result<UploadBodyStats, &'static str> {
    match forward_upload_body::<H>(socket, chunk_buf, prefetched, content_length).await {
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
                H::record_upload_http_read_body_reset();
            }
            log_upload_body_read_error::<H>(socket, err, consumed, content_length, pending, want);
            if abort_on_error {
                abort_upload_roundtrip_bounded::<H>(if reset {
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
                abort_upload_roundtrip_bounded::<H>("incomplete_body").await;
            }
            write_response(socket, b"400 Bad Request", b"incomplete body").await;
            Err("incomplete body")
        }
        Err(UploadBodyError::Roundtrip(err)) => {
            if abort_on_error {
                abort_upload_roundtrip_bounded::<H>("roundtrip_err").await;
            }
            write_roundtrip_error_response::<H>(socket, err).await;
            Err(H::error_log(err))
        }
    }
}

async fn forward_upload_body<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    prefetched: &[u8],
    content_length: usize,
) -> Result<UploadBodyStats, UploadBodyError<H::Error>> {
    let mut consumed = 0usize;
    let mut pending = 0usize;
    let mut progress = UploadBodyProgress::new();
    let mut ingress_adapt = IngressFairnessAdaptive::new(
        H::HTTP_INGRESS_ADAPTIVE_FAIRNESS,
        H::HTTP_INGRESS_COOP_YIELD_BYTES,
        H::HTTP_INGRESS_COOP_YIELD_READS,
    );
    let mut inflight: Option<InflightChunk<H::Inflight>> = None;

    let mut prefetched_offset = 0usize;
    while prefetched_offset < prefetched.len() && consumed < content_length {
        let free = chunk_buf.len().saturating_sub(pending);
        let copy_len = min(free, prefetched.len() - prefetched_offset);
        let copy_started_at = embassy_time::Instant::now();
        chunk_buf[pending..pending + copy_len]
            .copy_from_slice(&prefetched[prefetched_offset..prefetched_offset + copy_len]);
        progress.record_payload_copy_ms(elapsed_ms_u32(copy_started_at));
        pending += copy_len;
        consumed += copy_len;
        prefetched_offset += copy_len;

        if pending == chunk_buf.len() || consumed == content_length {
            queue_chunk_for_sd::<H>(&chunk_buf[..pending], &mut inflight, &mut progress).await?;
            pending = 0;
        }
    }

    while consumed < content_length {
        let pre_read_queue = usize_to_u32_saturating(socket.recv_queue());
        if progress.should_try_drain(pre_read_queue, H::HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS) {
            try_drain_inflight_chunk::<H>(&mut inflight, &mut progress)?;
            progress.reset_try_drain_counter();
        }
        let want = min(
            chunk_buf.len().saturating_sub(pending),
            content_length - consumed,
        );
        progress.note_pre_read(pre_read_queue);
        let read_started_at = embassy_time::Instant::now();
        let n = match socket.read(&mut chunk_buf[pending..pending + want]).await {
            Ok(n) => n,
            Err(err) => {
                drain_inflight_on_error::<H>(&mut inflight).await;
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
        progress.note_read_result(pre_read_queue, read_wait_ms, n, want, &mut ingress_adapt);
        if n == 0 {
            drain_inflight_on_error::<H>(&mut inflight).await;
            return Err(UploadBodyError::IncompleteBody);
        }

        pending += n;
        consumed += n;

        if pending == chunk_buf.len() || consumed == content_length {
            queue_chunk_for_sd::<H>(&chunk_buf[..pending], &mut inflight, &mut progress).await?;
            pending = 0;
        }

        if progress.should_yield(&ingress_adapt) {
            // Keep the net runner from being starved by long, immediately-ready
            // read bursts in this cooperative executor.
            yield_now().await;
            progress.reset_yield_counters();
        }
    }
    flush_inflight_chunk::<H>(&mut inflight, &mut progress).await?;
    Ok(progress.finish(ingress_adapt.snapshot()))
}
