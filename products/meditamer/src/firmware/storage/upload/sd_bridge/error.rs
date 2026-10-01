use crate::firmware::types::SdUploadResultCode;

#[derive(Clone, Copy)]
pub(crate) enum SdUploadRoundtripError {
    Timeout,
    Device(SdUploadResultCode),
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
            SdUploadResultCode::DirectoryFull => "sd directory entries full",
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
            SdUploadResultCode::DirectoryFull => b"507 Insufficient Storage",
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
            SdUploadResultCode::DirectoryFull => b"sd directory entries full",
            SdUploadResultCode::OperationFailed => b"sd operation failed",
        },
    }
}
