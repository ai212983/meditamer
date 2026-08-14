use embassy_time::Instant;
use sdcard::probe::SdWriteMetrics;

use super::super::super::super::types::{SdUploadCommand, SD_PATH_MAX};
use super::super::SD_UPLOAD_PATH_BUF_MAX;

#[derive(Clone, Copy, Debug, Default)]
pub(in super::super) struct SdUploadChunkTimingMetrics {
    pub(super) chunk_count: u32,
    pub(super) chunk_queue_wait_ms_total: u32,
    pub(super) chunk_queue_wait_ms_max: u32,
    pub(super) chunk_total_ms_total: u32,
    pub(super) chunk_total_ms_max: u32,
    pub(super) chunk_total_over_200ms: u32,
    pub(super) chunk_total_over_400ms: u32,
    pub(super) ensure_ready_ms_total: u32,
    pub(super) ensure_ready_ms_max: u32,
    pub(super) payload_lock_ms_total: u32,
    pub(super) payload_lock_ms_max: u32,
    pub(super) append_total_ms_total: u32,
    pub(super) append_total_ms_max: u32,
    pub(super) append_total_over_200ms: u32,
    pub(super) append_total_over_400ms: u32,
    pub(super) append_capacity_ms_total: u32,
    pub(super) append_capacity_ms_max: u32,
    pub(super) append_write_data_ms_total: u32,
    pub(super) append_write_data_ms_max: u32,
    pub(super) chunk_non_append_ms_total: u32,
    pub(super) chunk_non_append_ms_max: u32,
    pub(super) chunk_residual_ms_total: u32,
    pub(super) chunk_residual_ms_max: u32,
    pub(super) chunk_overhead_ms_total: u32,
    pub(super) chunk_overhead_ms_max: u32,
}

pub(super) struct SdUploadChunkTimingSample {
    pub(super) queue_wait_ms: u32,
    pub(super) total_ms: u32,
    pub(super) ensure_ready_ms: u32,
    pub(super) payload_lock_ms: u32,
    pub(super) append_total_ms: u32,
    pub(super) append_capacity_ms: u32,
    pub(super) append_write_data_ms: u32,
}

impl SdUploadChunkTimingMetrics {
    pub(super) fn record_chunk(&mut self, sample: SdUploadChunkTimingSample) {
        let SdUploadChunkTimingSample {
            queue_wait_ms: chunk_queue_wait_ms,
            total_ms: chunk_total_ms,
            ensure_ready_ms,
            payload_lock_ms,
            append_total_ms,
            append_capacity_ms,
            append_write_data_ms,
        } = sample;
        self.chunk_count = self.chunk_count.saturating_add(1);
        self.chunk_queue_wait_ms_total = self
            .chunk_queue_wait_ms_total
            .saturating_add(chunk_queue_wait_ms);
        self.chunk_queue_wait_ms_max = self.chunk_queue_wait_ms_max.max(chunk_queue_wait_ms);
        self.chunk_total_ms_total = self.chunk_total_ms_total.saturating_add(chunk_total_ms);
        self.chunk_total_ms_max = self.chunk_total_ms_max.max(chunk_total_ms);
        if chunk_total_ms >= 200 {
            self.chunk_total_over_200ms = self.chunk_total_over_200ms.saturating_add(1);
        }
        if chunk_total_ms >= 400 {
            self.chunk_total_over_400ms = self.chunk_total_over_400ms.saturating_add(1);
        }

        self.ensure_ready_ms_total = self.ensure_ready_ms_total.saturating_add(ensure_ready_ms);
        self.ensure_ready_ms_max = self.ensure_ready_ms_max.max(ensure_ready_ms);

        self.payload_lock_ms_total = self.payload_lock_ms_total.saturating_add(payload_lock_ms);
        self.payload_lock_ms_max = self.payload_lock_ms_max.max(payload_lock_ms);

        self.append_total_ms_total = self.append_total_ms_total.saturating_add(append_total_ms);
        self.append_total_ms_max = self.append_total_ms_max.max(append_total_ms);
        if append_total_ms >= 200 {
            self.append_total_over_200ms = self.append_total_over_200ms.saturating_add(1);
        }
        if append_total_ms >= 400 {
            self.append_total_over_400ms = self.append_total_over_400ms.saturating_add(1);
        }

        self.append_capacity_ms_total = self
            .append_capacity_ms_total
            .saturating_add(append_capacity_ms);
        self.append_capacity_ms_max = self.append_capacity_ms_max.max(append_capacity_ms);
        self.append_write_data_ms_total = self
            .append_write_data_ms_total
            .saturating_add(append_write_data_ms);
        self.append_write_data_ms_max = self.append_write_data_ms_max.max(append_write_data_ms);

        let chunk_non_append_ms = chunk_total_ms.saturating_sub(append_total_ms);
        self.chunk_non_append_ms_total = self
            .chunk_non_append_ms_total
            .saturating_add(chunk_non_append_ms);
        self.chunk_non_append_ms_max = self.chunk_non_append_ms_max.max(chunk_non_append_ms);
        let chunk_residual_ms = chunk_queue_wait_ms.saturating_add(chunk_non_append_ms);
        self.chunk_residual_ms_total = self
            .chunk_residual_ms_total
            .saturating_add(chunk_residual_ms);
        self.chunk_residual_ms_max = self.chunk_residual_ms_max.max(chunk_residual_ms);

        let accounted_ms = ensure_ready_ms
            .saturating_add(payload_lock_ms)
            .saturating_add(append_total_ms);
        let chunk_overhead_ms = chunk_total_ms.saturating_sub(accounted_ms);
        self.chunk_overhead_ms_total = self
            .chunk_overhead_ms_total
            .saturating_add(chunk_overhead_ms);
        self.chunk_overhead_ms_max = self.chunk_overhead_ms_max.max(chunk_overhead_ms);
    }
}

pub(in super::super) struct SdUploadSession {
    pub(super) final_path: [u8; SD_UPLOAD_PATH_BUF_MAX],
    pub(super) final_path_len: u8,
    pub(super) temp_path: [u8; SD_UPLOAD_PATH_BUF_MAX],
    pub(super) temp_path_len: u8,
    pub(super) expected_size: u32,
    pub(super) bytes_written: u32,
    pub(super) last_activity_at: Instant,
    pub(super) write_metrics_start: SdWriteMetrics,
    pub(super) chunk_timing: SdUploadChunkTimingMetrics,
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
