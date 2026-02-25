use embassy_time::{with_timeout, Duration};

use super::super::{
    config::{SD_UPLOAD_REQUESTS, SD_UPLOAD_RESULTS},
    types::{
        SdUploadCommand, SdUploadRequest, SdUploadResult, SdUploadResultCode, SD_UPLOAD_CHUNK_MAX,
    },
};

const SD_UPLOAD_RESPONSE_TIMEOUT_MS: u64 = 10_000;

#[derive(Clone, Copy)]
pub(crate) enum SdUploadRoundtripError {
    Timeout,
    Device(SdUploadResultCode),
}

pub(crate) async fn sd_upload_chunk(data: &[u8]) -> Result<SdUploadResult, SdUploadRoundtripError> {
    if data.len() > SD_UPLOAD_CHUNK_MAX {
        return Err(SdUploadRoundtripError::Device(
            SdUploadResultCode::OperationFailed,
        ));
    }
    let mut payload = [0u8; SD_UPLOAD_CHUNK_MAX];
    payload[..data.len()].copy_from_slice(data);
    sd_upload_roundtrip_raw(SdUploadCommand::Chunk {
        data: payload,
        data_len: data.len() as u16,
    })
    .await
}

pub(crate) async fn sd_upload_roundtrip(
    command: SdUploadCommand,
) -> Result<SdUploadResult, SdUploadRoundtripError> {
    sd_upload_roundtrip_raw(command).await
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

async fn sd_upload_roundtrip_raw(
    command: SdUploadCommand,
) -> Result<SdUploadResult, SdUploadRoundtripError> {
    SD_UPLOAD_REQUESTS.send(SdUploadRequest { command }).await;
    let result = with_timeout(
        Duration::from_millis(SD_UPLOAD_RESPONSE_TIMEOUT_MS),
        SD_UPLOAD_RESULTS.receive(),
    )
    .await
    .map_err(|_| SdUploadRoundtripError::Timeout)?;
    if result.ok {
        Ok(result)
    } else {
        Err(SdUploadRoundtripError::Device(result.code))
    }
}
