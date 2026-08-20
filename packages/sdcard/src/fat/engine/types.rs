use super::super::{FatDirEntry, SdFatError};
use crate::{probe::SdProbeError, SD_PATH_MAX};

pub const FAT_ENGINE_LIST_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatBufferId {
    Sector,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatPayloadId {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatIoAction {
    ReadSector {
        lba: u32,
        buffer: FatBufferId,
    },
    WriteSector {
        lba: u32,
        buffer: FatBufferId,
    },
    ReadSectorToPayload {
        lba: u32,
        buffer: FatBufferId,
        payload: FatPayloadId,
        payload_offset: u32,
        len: u16,
    },
    WriteSectorFromPayload {
        lba: u32,
        buffer: FatBufferId,
        payload: FatPayloadId,
        payload_offset: u32,
        sector_offset: u16,
        len: u16,
        preserve_existing: bool,
    },
    WritePayloadSectors {
        start_lba: u32,
        payload: FatPayloadId,
        payload_offset: u32,
        sectors: u16,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum FatRequest {
    List {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    Read {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        output: FatPayloadId,
        output_capacity: u32,
    },
    /// Like [`FatRequest::Read`], but for files that need not fit in any
    /// single buffer: the caller drains one [`crate::probe::SD_SECTOR_SIZE`]
    /// chunk at a time via [`super::FatEngine::stream_bytes_delivered`] /
    /// `stream_chunk_len` instead of receiving the whole file at once. See
    /// docs/plans/single-production-sd-recovery-updater.md (Phase 1).
    Stream {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    Write {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        input: FatPayloadId,
        input_len: u32,
    },
    Stat {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    Mkdir {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    Remove {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    Rename {
        src_path: [u8; SD_PATH_MAX],
        src_path_len: u8,
        dst_path: [u8; SD_PATH_MAX],
        dst_path_len: u8,
        replace: bool,
    },
    Append {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        input: FatPayloadId,
        input_len: u32,
    },
    Truncate {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        size: u32,
    },
    UploadBegin {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        expected_size: u32,
    },
    UploadChunk {
        input: FatPayloadId,
        input_len: u32,
    },
    UploadCommit {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    UploadFlush,
    UploadClear,
}

impl FatRequest {
    pub(super) fn path(&self) -> (&[u8; SD_PATH_MAX], u8) {
        match self {
            Self::List { path, path_len }
            | Self::Read { path, path_len, .. }
            | Self::Stream { path, path_len }
            | Self::Write { path, path_len, .. }
            | Self::Stat { path, path_len }
            | Self::Mkdir { path, path_len }
            | Self::Remove { path, path_len }
            | Self::Append { path, path_len, .. }
            | Self::Truncate { path, path_len, .. } => (path, *path_len),
            Self::UploadBegin { path, path_len, .. } | Self::UploadCommit { path, path_len } => {
                (path, *path_len)
            }
            Self::Rename {
                src_path,
                src_path_len,
                ..
            } => (src_path, *src_path_len),
            Self::UploadChunk { .. } | Self::UploadFlush | Self::UploadClear => {
                static EMPTY: [u8; SD_PATH_MAX] = [0; SD_PATH_MAX];
                (&EMPTY, 0)
            }
        }
    }
}

#[derive(Debug)]
pub enum FatEngineError {
    Busy,
    NotStarted,
    MissingIoCompletion,
    UnexpectedIoCompletion,
    InvalidState,
    UnsupportedRequest,
    TimedOut,
    Io(SdProbeError),
    Fat(SdFatError),
}

impl FatEngineError {
    pub const fn is_transport_failure(&self) -> bool {
        matches!(self, Self::TimedOut | Self::Io(_))
    }
}

#[derive(Debug)]
pub enum FatResult {
    Done,
    Listed { count: u8 },
    Read { bytes: u32 },
    Streamed { bytes: u32 },
    Stat(FatDirEntry),
    Error(FatEngineError),
}

#[derive(Debug)]
pub enum FatIoCompletion {
    Pending,
    Done,
    Failed(SdProbeError),
    TimedOut,
    InvalidState,
}

#[derive(Debug)]
pub enum FatStep {
    Io(FatIoAction),
    Continue,
    Yield,
    Complete(FatResult),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatStageLabel {
    Idle,
    MountMbr,
    MountBoot,
    ResolvePath,
    ScanDirectory,
    ReadFat,
    ListDirectory,
    ReadFile,
    StreamFile,
    WriteFile,
    Allocate,
    FreeChain,
    UpdateDirectory,
    Complete,
}

pub struct SdWorkspace {
    pub sector: [u8; crate::probe::SD_SECTOR_SIZE],
    pub entry: FatDirEntry,
    pub(super) segments: [super::super::PathSegment; super::super::MAX_PATH_SEGMENTS],
    pub(super) segment_count: u8,
}

impl SdWorkspace {
    pub const fn new() -> Self {
        Self {
            sector: [0; crate::probe::SD_SECTOR_SIZE],
            entry: FatDirEntry::EMPTY,
            segments: [super::super::PathSegment::EMPTY; super::super::MAX_PATH_SEGMENTS],
            segment_count: 0,
        }
    }

    pub(super) fn reset_operation(&mut self) {
        self.entry = FatDirEntry::EMPTY;
        self.segments.fill(super::super::PathSegment::EMPTY);
        self.segment_count = 0;
    }
}

impl Default for SdWorkspace {
    fn default() -> Self {
        Self::new()
    }
}
