use embassy_time::Instant;
use sdcard::fat;
use sdcard::probe::SdWriteMetrics;

use super::super::super::super::types::{SdUploadCommand, SD_PATH_MAX};
use super::super::SD_UPLOAD_PATH_BUF_MAX;

pub(in super::super) struct SdUploadSession {
    pub(super) final_path: [u8; SD_UPLOAD_PATH_BUF_MAX],
    pub(super) final_path_len: u8,
    pub(super) temp_path: [u8; SD_UPLOAD_PATH_BUF_MAX],
    pub(super) temp_path_len: u8,
    pub(super) append_session: fat::FatAppendSession,
    pub(super) expected_size: u32,
    pub(super) bytes_written: u32,
    pub(super) last_activity_at: Instant,
    pub(super) write_metrics_start: SdWriteMetrics,
}

pub(super) enum UploadCommandGroup {
    Stream(UploadStreamCommand),
    Path(UploadPathCommand),
}

pub(super) enum UploadStreamCommand {
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
}

pub(super) enum UploadPathCommand {
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

pub(super) fn split_upload_command(command: SdUploadCommand) -> UploadCommandGroup {
    match command {
        SdUploadCommand::Begin {
            path,
            path_len,
            expected_size,
        } => UploadCommandGroup::Stream(UploadStreamCommand::Begin {
            path,
            path_len,
            expected_size,
        }),
        SdUploadCommand::Chunk { data_len } => {
            UploadCommandGroup::Stream(UploadStreamCommand::Chunk { data_len })
        }
        SdUploadCommand::Commit => UploadCommandGroup::Stream(UploadStreamCommand::Commit),
        SdUploadCommand::Abort => UploadCommandGroup::Stream(UploadStreamCommand::Abort),
        SdUploadCommand::Mkdir { path, path_len } => {
            UploadCommandGroup::Path(UploadPathCommand::Mkdir { path, path_len })
        }
        SdUploadCommand::Remove { path, path_len } => {
            UploadCommandGroup::Path(UploadPathCommand::Remove { path, path_len })
        }
        SdUploadCommand::Stat { path, path_len } => {
            UploadCommandGroup::Path(UploadPathCommand::Stat { path, path_len })
        }
    }
}
