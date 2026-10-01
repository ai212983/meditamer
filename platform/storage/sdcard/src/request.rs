use crate::fat::{FatPayloadId, FatRequest};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageCommand {
    Probe,
    RwVerify {
        lba: u32,
    },
    FatList {
        path: [u8; crate::SD_PATH_MAX],
        path_len: u8,
    },
    FatRead {
        path: [u8; crate::SD_PATH_MAX],
        path_len: u8,
    },
    FatWrite {
        path: [u8; crate::SD_PATH_MAX],
        path_len: u8,
        data: [u8; crate::SD_WRITE_MAX],
        data_len: u16,
    },
    FatStat {
        path: [u8; crate::SD_PATH_MAX],
        path_len: u8,
    },
    FatMkdir {
        path: [u8; crate::SD_PATH_MAX],
        path_len: u8,
    },
    FatRemove {
        path: [u8; crate::SD_PATH_MAX],
        path_len: u8,
    },
    FatRename {
        src_path: [u8; crate::SD_PATH_MAX],
        src_path_len: u8,
        dst_path: [u8; crate::SD_PATH_MAX],
        dst_path_len: u8,
    },
    FatAppend {
        path: [u8; crate::SD_PATH_MAX],
        path_len: u8,
        data: [u8; crate::SD_WRITE_MAX],
        data_len: u16,
    },
    FatTruncate {
        path: [u8; crate::SD_PATH_MAX],
        path_len: u8,
        size: u32,
    },
}

pub type SdCommand = StorageCommand;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdCommandKind {
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
pub struct SdRequest {
    pub id: u32,
    pub command: SdCommand,
}

impl StorageCommand {
    pub fn kind(self) -> SdCommandKind {
        match self {
            Self::Probe => SdCommandKind::Probe,
            Self::RwVerify { .. } => SdCommandKind::RwVerify,
            Self::FatList { .. } => SdCommandKind::FatList,
            Self::FatRead { .. } => SdCommandKind::FatRead,
            Self::FatWrite { .. } => SdCommandKind::FatWrite,
            Self::FatStat { .. } => SdCommandKind::FatStat,
            Self::FatMkdir { .. } => SdCommandKind::FatMkdir,
            Self::FatRemove { .. } => SdCommandKind::FatRemove,
            Self::FatRename { .. } => SdCommandKind::FatRename,
            Self::FatAppend { .. } => SdCommandKind::FatAppend,
            Self::FatTruncate { .. } => SdCommandKind::FatTruncate,
        }
    }

    pub fn fat_request(self, output_capacity: u32) -> Option<FatRequest> {
        Some(match self {
            Self::FatList { path, path_len } => FatRequest::List { path, path_len },
            Self::FatRead { path, path_len } => FatRequest::Read {
                path,
                path_len,
                output: FatPayloadId::Primary,
                output_capacity,
            },
            Self::FatWrite {
                path,
                path_len,
                data_len,
                ..
            } => FatRequest::Write {
                path,
                path_len,
                input: FatPayloadId::Primary,
                input_len: u32::from(data_len).min(crate::SD_WRITE_MAX as u32),
            },
            Self::FatStat { path, path_len } => FatRequest::Stat { path, path_len },
            Self::FatMkdir { path, path_len } => FatRequest::Mkdir { path, path_len },
            Self::FatRemove { path, path_len } => FatRequest::Remove { path, path_len },
            Self::FatRename {
                src_path,
                src_path_len,
                dst_path,
                dst_path_len,
            } => FatRequest::Rename {
                src_path,
                src_path_len,
                dst_path,
                dst_path_len,
                replace: false,
            },
            Self::FatAppend {
                path,
                path_len,
                data_len,
                ..
            } => FatRequest::Append {
                path,
                path_len,
                input: FatPayloadId::Primary,
                input_len: u32::from(data_len).min(crate::SD_WRITE_MAX as u32),
            },
            Self::FatTruncate {
                path,
                path_len,
                size,
            } => FatRequest::Truncate {
                path,
                path_len,
                size,
            },
            Self::Probe | Self::RwVerify { .. } => return None,
        })
    }

    pub fn input_payload(&self) -> &[u8] {
        match self {
            Self::FatWrite { data, data_len, .. } | Self::FatAppend { data, data_len, .. } => {
                &data[..usize::from(*data_len).min(data.len())]
            }
            _ => &[],
        }
    }
}
