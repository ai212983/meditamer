use super::{SD_ASSET_READ_MAX, SD_PATH_MAX, SD_UPLOAD_CHUNK_MAX, SD_WRITE_MAX};

#[derive(Clone, Copy)]
pub(crate) enum StorageCommand {
    Probe,
    RwVerify {
        lba: u32,
    },
    FatList {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRead {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatWrite {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        data: [u8; SD_WRITE_MAX],
        data_len: u16,
    },
    FatStat {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatMkdir {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRemove {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRename {
        src_path: [u8; SD_PATH_MAX],
        src_path_len: u8,
        dst_path: [u8; SD_PATH_MAX],
        dst_path_len: u8,
    },
    FatAppend {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        data: [u8; SD_WRITE_MAX],
        data_len: u16,
    },
    FatTruncate {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        size: u32,
    },
}

pub(crate) type SdCommand = StorageCommand;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SdCommandKind {
    Probe,
    RwVerify,
    FatList,
    FatRead,
    FatWrite,
    FatStat,
    FatMkdir,
    FatRemove,
    FatRename,
    FatAppend,
    FatTruncate,
}

#[derive(Clone, Copy)]
pub(crate) struct SdRequest {
    pub(crate) id: u32,
    pub(crate) command: SdCommand,
}

#[derive(Clone, Copy)]
pub(crate) struct SdResult {
    pub(crate) id: u32,
    pub(crate) kind: SdCommandKind,
    pub(crate) ok: bool,
    pub(crate) code: SdResultCode,
    pub(crate) attempts: u8,
    pub(crate) duration_ms: u32,
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
        data_len: u16,
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
}

pub(crate) struct SdUploadRequest {
    pub(crate) command: SdUploadCommand,
    pub(crate) chunk_data: Option<[u8; SD_UPLOAD_CHUNK_MAX]>,
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
    OperationFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SdUploadResult {
    pub(crate) ok: bool,
    pub(crate) code: SdUploadResultCode,
    pub(crate) bytes_written: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct SdAssetReadRequest {
    pub(crate) path: [u8; SD_PATH_MAX],
    pub(crate) path_len: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SdAssetReadResultCode {
    Ok,
    Busy,
    InvalidPath,
    NotFound,
    SizeMismatch,
    PowerOnFailed,
    InitFailed,
    OperationFailed,
}

#[derive(Clone, Copy)]
pub(crate) struct SdAssetReadResponse {
    pub(crate) ok: bool,
    pub(crate) code: SdAssetReadResultCode,
    pub(crate) data: [u8; SD_ASSET_READ_MAX],
    pub(crate) data_len: u16,
}

#[derive(Clone, Copy)]
pub(crate) enum SdPowerRequest {
    On,
    Off,
}
