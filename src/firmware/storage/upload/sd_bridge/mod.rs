use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};
use embassy_time::{with_timeout, Duration, Instant};

use super::super::super::{
    config::{SD_UPLOAD_REQUESTS, SD_UPLOAD_RESULTS},
    storage::transfer_buffers,
    telemetry,
    types::{
        SdUploadCommand, SdUploadRequest, SdUploadResult, SdUploadResultCode, SD_UPLOAD_CHUNK_MAX,
    },
};

mod error;

pub(crate) use error::{
    roundtrip_error_body, roundtrip_error_log, roundtrip_error_status, SdUploadRoundtripError,
};

const SD_UPLOAD_RESPONSE_TIMEOUT_MS: u64 = 10_000;
static SD_UPLOAD_ROUNDTRIP_LOCK: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

pub(crate) struct SdUploadChunkInFlight {
    pub(crate) copy_ms: u32,
    started_at: Instant,
    _lock: MutexGuard<'static, CriticalSectionRawMutex, ()>,
}

pub(crate) enum SdUploadChunkTryFinish {
    Pending(SdUploadChunkInFlight),
    Finished(SdUploadChunkFinish),
}

pub(crate) struct SdUploadChunkFinish {
    pub(crate) roundtrip_ms: u32,
    pub(crate) queue_wait_ms: u32,
    pub(crate) handler_ms: u32,
    pub(crate) post_handler_ms: u32,
    pub(crate) publish_to_receive_ms: u32,
}

pub(crate) async fn sd_upload_chunk_start(
    data: &[u8],
) -> Result<SdUploadChunkInFlight, SdUploadRoundtripError> {
    if data.len() > SD_UPLOAD_CHUNK_MAX {
        telemetry::record_sd_upload_roundtrip_code(SdUploadResultCode::OperationFailed);
        return Err(SdUploadRoundtripError::Device(
            SdUploadResultCode::OperationFailed,
        ));
    }
    let lock = SD_UPLOAD_ROUNDTRIP_LOCK.lock().await;
    drain_stale_sd_upload_results();
    let copy_started_at = Instant::now();
    {
        let mut payload = transfer_buffers::lock_upload_chunk_buffer()
            .await
            .map_err(|_| SdUploadRoundtripError::Device(SdUploadResultCode::OperationFailed))?;
        payload.as_mut_slice()[..data.len()].copy_from_slice(data);
    }
    let copy_ms = elapsed_ms_u32(copy_started_at);
    let started_at = Instant::now();
    SD_UPLOAD_REQUESTS
        .send(SdUploadRequest {
            command: SdUploadCommand::Chunk {
                data_len: data.len() as u32,
            },
            enqueued_at_ms: now_ms_u32(),
        })
        .await;
    Ok(SdUploadChunkInFlight {
        copy_ms,
        started_at,
        _lock: lock,
    })
}

pub(crate) async fn sd_upload_chunk_finish(
    inflight: SdUploadChunkInFlight,
) -> Result<SdUploadChunkFinish, SdUploadRoundtripError> {
    let started_at = inflight.started_at;
    let _lock = inflight._lock;
    let result = match receive_sd_upload_result_with_timeout(started_at).await {
        Some(result) => result,
        None => {
            drain_stale_sd_upload_results();
            let roundtrip_ms = elapsed_ms_u32(started_at);
            telemetry::record_sd_upload_roundtrip_timing(
                telemetry::SdUploadRoundtripPhase::Chunk,
                roundtrip_ms,
            );
            telemetry::record_sd_upload_roundtrip_timeout();
            return Err(SdUploadRoundtripError::Timeout);
        }
    };
    let roundtrip_ms = elapsed_ms_u32(started_at);
    telemetry::record_sd_upload_roundtrip_timing(
        telemetry::SdUploadRoundtripPhase::Chunk,
        roundtrip_ms,
    );

    if !result.ok {
        telemetry::record_sd_upload_roundtrip_code(result.code);
        return Err(SdUploadRoundtripError::Device(result.code));
    }
    let receive_at_ms = now_ms_u32();
    let publish_to_receive_ms = if result.chunk_published_at_ms == 0 {
        0
    } else {
        receive_at_ms.wrapping_sub(result.chunk_published_at_ms)
    };

    Ok(SdUploadChunkFinish {
        roundtrip_ms,
        queue_wait_ms: result.chunk_queue_wait_ms,
        handler_ms: result.chunk_handler_ms,
        post_handler_ms: result.chunk_post_handler_ms,
        publish_to_receive_ms,
    })
}

pub(crate) fn sd_upload_chunk_try_finish(
    inflight: SdUploadChunkInFlight,
) -> Result<SdUploadChunkTryFinish, SdUploadRoundtripError> {
    let started_at = inflight.started_at;
    let Ok(result) = SD_UPLOAD_RESULTS.try_receive() else {
        return Ok(SdUploadChunkTryFinish::Pending(inflight));
    };
    telemetry::record_sd_upload_roundtrip_timing(
        telemetry::SdUploadRoundtripPhase::Chunk,
        elapsed_ms_u32(started_at),
    );
    if !result.ok {
        telemetry::record_sd_upload_roundtrip_code(result.code);
        return Err(SdUploadRoundtripError::Device(result.code));
    }
    let receive_at_ms = now_ms_u32();
    let publish_to_receive_ms = if result.chunk_published_at_ms == 0 {
        0
    } else {
        receive_at_ms.wrapping_sub(result.chunk_published_at_ms)
    };
    Ok(SdUploadChunkTryFinish::Finished(SdUploadChunkFinish {
        roundtrip_ms: elapsed_ms_u32(started_at),
        queue_wait_ms: result.chunk_queue_wait_ms,
        handler_ms: result.chunk_handler_ms,
        post_handler_ms: result.chunk_post_handler_ms,
        publish_to_receive_ms,
    }))
}

pub(crate) async fn sd_upload_roundtrip(
    command: SdUploadCommand,
) -> Result<SdUploadResult, SdUploadRoundtripError> {
    let _lock = SD_UPLOAD_ROUNDTRIP_LOCK.lock().await;
    sd_upload_roundtrip_raw_locked(command).await
}

fn drain_stale_sd_upload_results() {
    while SD_UPLOAD_RESULTS.try_receive().is_ok() {}
}

async fn sd_upload_roundtrip_raw_locked(
    command: SdUploadCommand,
) -> Result<SdUploadResult, SdUploadRoundtripError> {
    let phase = phase_for_command(&command);
    // A previous request may have timed out locally while the SD task still produced
    // a late result. Drain any queued stale responses before issuing a new request.
    drain_stale_sd_upload_results();

    let started_at = Instant::now();
    SD_UPLOAD_REQUESTS
        .send(SdUploadRequest {
            command,
            enqueued_at_ms: now_ms_u32(),
        })
        .await;

    let result = match receive_sd_upload_result_with_timeout(started_at).await {
        Some(result) => result,
        None => {
            // If a response raced with timeout handling, clear it so the next
            // roundtrip cannot consume a stale result.
            drain_stale_sd_upload_results();
            telemetry::record_sd_upload_roundtrip_timing(phase, elapsed_ms_u32(started_at));
            telemetry::record_sd_upload_roundtrip_timeout();
            return Err(SdUploadRoundtripError::Timeout);
        }
    };
    telemetry::record_sd_upload_roundtrip_timing(phase, elapsed_ms_u32(started_at));

    if result.ok {
        Ok(result)
    } else {
        telemetry::record_sd_upload_roundtrip_code(result.code);
        Err(SdUploadRoundtripError::Device(result.code))
    }
}

async fn receive_sd_upload_result_with_timeout(started_at: Instant) -> Option<SdUploadResult> {
    if let Ok(result) = SD_UPLOAD_RESULTS.try_receive() {
        return Some(result);
    }

    let remaining_ms =
        SD_UPLOAD_RESPONSE_TIMEOUT_MS.saturating_sub(started_at.elapsed().as_millis());
    if remaining_ms == 0 {
        return None;
    }

    with_timeout(
        Duration::from_millis(remaining_ms),
        SD_UPLOAD_RESULTS.receive(),
    )
    .await
    .ok()
}

fn phase_for_command(command: &SdUploadCommand) -> telemetry::SdUploadRoundtripPhase {
    match command {
        SdUploadCommand::Begin { .. } => telemetry::SdUploadRoundtripPhase::Begin,
        SdUploadCommand::Chunk { .. } => telemetry::SdUploadRoundtripPhase::Chunk,
        SdUploadCommand::Commit => telemetry::SdUploadRoundtripPhase::Commit,
        SdUploadCommand::Abort => telemetry::SdUploadRoundtripPhase::Abort,
        SdUploadCommand::Mkdir { .. } => telemetry::SdUploadRoundtripPhase::Mkdir,
        SdUploadCommand::Remove { .. } => telemetry::SdUploadRoundtripPhase::Remove,
        SdUploadCommand::Stat { .. } => telemetry::SdUploadRoundtripPhase::Remove,
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

fn now_ms_u32() -> u32 {
    let now_ms = Instant::now().as_millis();
    if now_ms > u32::MAX as u64 {
        u32::MAX
    } else {
        now_ms as u32
    }
}
