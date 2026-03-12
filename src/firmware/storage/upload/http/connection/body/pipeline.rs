use embassy_time::Instant;

use super::super::super::super::sd_bridge::{
    sd_upload_chunk_finish, sd_upload_chunk_start, sd_upload_chunk_try_finish,
    SdUploadChunkInFlight, SdUploadChunkTryFinish,
};
use super::error::UploadBodyError;
use super::progress::{UploadBodyProgress, UPLOAD_CHUNK_PIPELINE_ENABLED};
use super::stats::elapsed_ms_u32;

pub(super) struct InflightChunk {
    transfer: SdUploadChunkInFlight,
    len: usize,
    queue_ms: u32,
    copy_ms: u32,
}

pub(super) async fn queue_chunk_for_sd(
    data: &[u8],
    inflight: &mut Option<InflightChunk>,
    progress: &mut UploadBodyProgress,
) -> Result<(), UploadBodyError> {
    let ingress_flush_started_at = Instant::now();
    flush_inflight_chunk(inflight, progress).await?;
    progress.record_ingress_flush_wait(elapsed_ms_u32(ingress_flush_started_at));

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
        flush_inflight_chunk(inflight, progress).await?;
        progress.record_ingress_flush_wait(elapsed_ms_u32(queue_started_at));
    }
    Ok(())
}

pub(super) async fn flush_inflight_chunk(
    inflight: &mut Option<InflightChunk>,
    progress: &mut UploadBodyProgress,
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
    progress.apply_finished_chunk(len, queue_ms, copy_ms, chunk_finish);
    Ok(())
}

pub(super) fn try_drain_inflight_chunk(
    inflight: &mut Option<InflightChunk>,
    progress: &mut UploadBodyProgress,
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
            progress.apply_finished_chunk(len, queue_ms, copy_ms, chunk_finish);
        }
    }
    Ok(())
}

pub(super) async fn drain_inflight_on_error(inflight: &mut Option<InflightChunk>) {
    let Some(inflight_chunk) = inflight.take() else {
        return;
    };
    let _ = sd_upload_chunk_finish(inflight_chunk.transfer).await;
}
