use super::SD_PATH_MAX;
pub(crate) use sdcard::request::{SdCommand, SdCommandKind, SdRequest};

#[derive(Clone, Copy)]
pub(crate) struct SdResult {
    pub(crate) id: u32,
    pub(crate) kind: SdCommandKind,
    pub(crate) ok: bool,
    pub(crate) code: SdResultCode,
    pub(crate) attempts: u8,
    pub(crate) duration_ms: u32,
    pub(crate) recover_bus: bool,
}

pub(crate) type SdResultCode = sdcard::runtime::SdRuntimeResultCode;

#[cfg_attr(not(feature = "asset-upload-http"), allow(dead_code))]
pub(crate) enum SdUploadCommand {
    Begin {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        expected_size: u32,
    },
    Chunk {
        data_len: u32,
    },
    Commit,
    Abort,
    Mkdir {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    Remove {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    Stat {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
}

pub(crate) struct SdUploadRequest {
    pub(crate) id: u32,
    pub(crate) command: SdUploadCommand,
    pub(crate) enqueued_at_ms: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SdUploadResultCode {
    Ok,
    Busy,
    SessionNotActive,
    InvalidPath,
    NotFound,
    NotEmpty,
    SizeMismatch,
    PowerOnFailed,
    InitFailed,
    DirectoryFull,
    OperationFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SdUploadResult {
    pub(crate) request_id: u32,
    pub(crate) ok: bool,
    pub(crate) code: SdUploadResultCode,
    pub(crate) bytes_written: u32,
    pub(crate) chunk_queue_wait_ms: u32,
    pub(crate) chunk_handler_ms: u32,
    pub(crate) chunk_post_handler_ms: u32,
    pub(crate) chunk_published_at_ms: u32,
    pub(crate) chunk_handler_done_at_ms: u32,
}

#[derive(Clone, Copy)]
pub(crate) enum SdPowerRequest {
    On,
    Off,
}
