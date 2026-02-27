use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Duration, Instant, Timer};

use super::super::super::{
    config::{SD_UPLOAD_REQUESTS, SD_UPLOAD_RESULTS},
    storage::transfer_buffers,
    telemetry,
    types::{
        SdUploadCommand, SdUploadRequest, SdUploadResult, SdUploadResultCode, SD_UPLOAD_CHUNK_MAX,
    },
};

const SD_UPLOAD_RESPONSE_TIMEOUT_MS: u64 = 10_000;
static SD_UPLOAD_ROUNDTRIP_LOCK: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

#[derive(Clone, Copy)]
pub(crate) enum SdUploadRoundtripError {
    Timeout,
    Device(SdUploadResultCode),
}

pub(crate) async fn sd_upload_chunk(data: &[u8]) -> Result<SdUploadResult, SdUploadRoundtripError> {
    if data.len() > SD_UPLOAD_CHUNK_MAX {
        telemetry::record_sd_upload_roundtrip_code(SdUploadResultCode::OperationFailed);
        return Err(SdUploadRoundtripError::Device(
            SdUploadResultCode::OperationFailed,
        ));
    }
    let _lock = SD_UPLOAD_ROUNDTRIP_LOCK.lock().await;
    {
        let mut payload = transfer_buffers::lock_upload_chunk_buffer()
            .await
            .map_err(|_| SdUploadRoundtripError::Device(SdUploadResultCode::OperationFailed))?;
        payload.as_mut_slice()[..data.len()].copy_from_slice(data);
    }
    sd_upload_roundtrip_raw_locked(SdUploadCommand::Chunk {
        data_len: data.len() as u16,
    })
    .await
}

pub(crate) async fn sd_upload_roundtrip(
    command: SdUploadCommand,
) -> Result<SdUploadResult, SdUploadRoundtripError> {
    let _lock = SD_UPLOAD_ROUNDTRIP_LOCK.lock().await;
    sd_upload_roundtrip_raw_locked(command).await
}

pub(crate) fn roundtrip_error_log(error: SdUploadRoundtripError) -> &'static str {
    match error {
        SdUploadRoundtripError::Timeout => "sd upload timeout",
        SdUploadRoundtripError::Device(code) => match code {
            SdUploadResultCode::Ok => "ok",
            SdUploadResultCode::Busy => "sd busy",
            SdUploadResultCode::SessionNotActive => "upload session not active",
            SdUploadResultCode::InvalidPath => "invalid path",
            SdUploadResultCode::NotFound => "not found",
            SdUploadResultCode::NotEmpty => "directory not empty",
            SdUploadResultCode::SizeMismatch => "size mismatch",
            SdUploadResultCode::PowerOnFailed => "sd power on failed",
            SdUploadResultCode::InitFailed => "sd init failed",
            SdUploadResultCode::OperationFailed => "sd operation failed",
        },
    }
}

pub(crate) fn roundtrip_error_status(error: SdUploadRoundtripError) -> &'static [u8] {
    match error {
        SdUploadRoundtripError::Timeout => b"504 Gateway Timeout",
        SdUploadRoundtripError::Device(code) => match code {
            SdUploadResultCode::Ok => b"200 OK",
            SdUploadResultCode::Busy => b"409 Conflict",
            SdUploadResultCode::SessionNotActive => b"409 Conflict",
            SdUploadResultCode::InvalidPath => b"400 Bad Request",
            SdUploadResultCode::NotFound => b"404 Not Found",
            SdUploadResultCode::NotEmpty => b"409 Conflict",
            SdUploadResultCode::SizeMismatch => b"400 Bad Request",
            SdUploadResultCode::PowerOnFailed => b"503 Service Unavailable",
            SdUploadResultCode::InitFailed => b"503 Service Unavailable",
            SdUploadResultCode::OperationFailed => b"500 Internal Server Error",
        },
    }
}

pub(crate) fn roundtrip_error_body(error: SdUploadRoundtripError) -> &'static [u8] {
    match error {
        SdUploadRoundtripError::Timeout => b"sd upload timeout",
        SdUploadRoundtripError::Device(code) => match code {
            SdUploadResultCode::Ok => b"ok",
            SdUploadResultCode::Busy => b"sd busy",
            SdUploadResultCode::SessionNotActive => b"upload session not active",
            SdUploadResultCode::InvalidPath => b"invalid path",
            SdUploadResultCode::NotFound => b"not found",
            SdUploadResultCode::NotEmpty => b"directory not empty",
            SdUploadResultCode::SizeMismatch => b"size mismatch",
            SdUploadResultCode::PowerOnFailed => b"sd power on failed",
            SdUploadResultCode::InitFailed => b"sd init failed",
            SdUploadResultCode::OperationFailed => b"sd operation failed",
        },
    }
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
    SD_UPLOAD_REQUESTS.send(SdUploadRequest { command }).await;

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
    loop {
        if let Ok(result) = SD_UPLOAD_RESULTS.try_receive() {
            return Some(result);
        }
        if started_at.elapsed() >= Duration::from_millis(SD_UPLOAD_RESPONSE_TIMEOUT_MS) {
            return None;
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}

fn phase_for_command(command: &SdUploadCommand) -> telemetry::SdUploadRoundtripPhase {
    match command {
        SdUploadCommand::Begin { .. } => telemetry::SdUploadRoundtripPhase::Begin,
        SdUploadCommand::Chunk { .. } => telemetry::SdUploadRoundtripPhase::Chunk,
        SdUploadCommand::Commit => telemetry::SdUploadRoundtripPhase::Commit,
        SdUploadCommand::Abort => telemetry::SdUploadRoundtripPhase::Abort,
        SdUploadCommand::Mkdir { .. } => telemetry::SdUploadRoundtripPhase::Mkdir,
        SdUploadCommand::Remove { .. } => telemetry::SdUploadRoundtripPhase::Remove,
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
