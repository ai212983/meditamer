use core::fmt::Write;

use sdcard::fat::{
    FatBufferId, FatEngine, FatIoAction, FatIoCompletion, FatPayloadId, FatStageLabel,
};

use super::super::super::super::types::SdProbeDriver;
use super::super::serial_log::{self, SdSerialLine};

macro_rules! queue_sd_line {
    ($($arg:tt)*) => {{
        let mut line = SdSerialLine::new();
        let _ = write!(&mut line, $($arg)*);
        let _ = line.push_str("\r\n");
        let _ = serial_log::send(line);
    }};
}

pub(super) async fn execute_action(
    action: FatIoAction,
    probe: &mut SdProbeDriver,
    engine: &mut FatEngine,
    input: &[u8],
    output: &mut [u8],
) -> FatIoCompletion {
    let result = match action {
        FatIoAction::ReadSector { lba, buffer } => {
            debug_assert_eq!(buffer, FatBufferId::Sector);
            probe
                .read_sector(lba, &mut engine.workspace_mut().sector)
                .await
        }
        FatIoAction::WriteSector { lba, buffer } => {
            debug_assert_eq!(buffer, FatBufferId::Sector);
            probe.write_sector(lba, &engine.workspace().sector).await
        }
        FatIoAction::ReadSectorToPayload {
            lba,
            buffer,
            payload,
            payload_offset,
            len,
        } => {
            debug_assert_eq!(buffer, FatBufferId::Sector);
            debug_assert_eq!(payload, FatPayloadId::Primary);
            let result = probe
                .read_sector(lba, &mut engine.workspace_mut().sector)
                .await;
            if result.is_ok() {
                let start = payload_offset as usize;
                let end = start.saturating_add(len as usize);
                if end > output.len() {
                    return FatIoCompletion::InvalidState;
                }
                output[start..end].copy_from_slice(&engine.workspace().sector[..len as usize]);
            }
            result
        }
        FatIoAction::WriteSectorFromPayload {
            lba,
            payload,
            payload_offset,
            sector_offset,
            len,
            preserve_existing,
            ..
        } => {
            debug_assert_eq!(payload, FatPayloadId::Primary);
            if !preserve_existing {
                engine.workspace_mut().sector.fill(0);
            }
            let src = payload_offset as usize;
            let dst = sector_offset as usize;
            let Some(src_end) = src.checked_add(len as usize) else {
                return FatIoCompletion::InvalidState;
            };
            let Some(dst_end) = dst.checked_add(len as usize) else {
                return FatIoCompletion::InvalidState;
            };
            if src_end > input.len() || dst_end > engine.workspace().sector.len() {
                return FatIoCompletion::InvalidState;
            }
            engine.workspace_mut().sector[dst..dst_end].copy_from_slice(&input[src..src_end]);
            probe.write_sector(lba, &engine.workspace().sector).await
        }
        FatIoAction::WritePayloadSectors {
            start_lba,
            payload,
            payload_offset,
            sectors,
        } => {
            debug_assert_eq!(payload, FatPayloadId::Primary);
            let start = payload_offset as usize;
            let Some(bytes) = (sectors as usize).checked_mul(sdcard::probe::SD_SECTOR_SIZE) else {
                return FatIoCompletion::InvalidState;
            };
            let Some(end) = start.checked_add(bytes) else {
                return FatIoCompletion::InvalidState;
            };
            if end > input.len() {
                return FatIoCompletion::InvalidState;
            }
            probe
                .write_sectors_contiguous(start_lba, &input[start..end])
                .await
        }
    };
    match result {
        Ok(()) => FatIoCompletion::Done,
        Err(err) if err.is_timeout() => {
            queue_sd_line!("sdfat[request]: transport_timeout err={:?}", err);
            FatIoCompletion::TimedOut
        }
        Err(err) => FatIoCompletion::Failed(err),
    }
}

pub(super) fn stage_before_tag(stage: FatStageLabel) -> &'static str {
    match stage {
        FatStageLabel::MountMbr | FatStageLabel::MountBoot => "sd_fat_mount_io_before",
        FatStageLabel::ResolvePath | FatStageLabel::ScanDirectory | FatStageLabel::ReadFat => {
            "sd_fat_metadata_io_before"
        }
        FatStageLabel::ReadFile | FatStageLabel::ListDirectory => "sd_fat_read_io_before",
        _ => "sd_fat_write_io_before",
    }
}

pub(super) fn stage_after_tag(stage: FatStageLabel) -> &'static str {
    match stage {
        FatStageLabel::MountMbr | FatStageLabel::MountBoot => "sd_fat_mount_io_after",
        FatStageLabel::ResolvePath | FatStageLabel::ScanDirectory | FatStageLabel::ReadFat => {
            "sd_fat_metadata_io_after"
        }
        FatStageLabel::ReadFile | FatStageLabel::ListDirectory => "sd_fat_read_io_after",
        _ => "sd_fat_write_io_after",
    }
}
