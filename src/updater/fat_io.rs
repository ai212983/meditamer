//! Generic [`sdcard::fat::FatIoAction`] executor shared by every FAT
//! operation the updater drives (`Stream` for reading the bundle and the
//! attempted-digest marker, `Write`/`Read` for the marker file itself).
//! Mirrors `src/firmware/storage/sd_task/engine_driver/io.rs::execute_action`
//! — the production firmware's equivalent — trimmed of the serial-log/
//! observability calls that module owns privately, plus the write-side
//! actions Phase 1's read-only `bundle_stream::execute_read_sector` didn't
//! need yet.

use sdcard::{
    fat::{FatEngine, FatIoAction, FatIoCompletion},
    probe::{SdCardProbe, SdProbeError, SdSpiBus},
    SD_PATH_MAX,
};

pub(super) fn encode_path(path: &str) -> Option<([u8; SD_PATH_MAX], u8)> {
    let bytes = path.as_bytes();
    if bytes.is_empty() || bytes.len() > SD_PATH_MAX {
        return None;
    }
    let mut out = [0u8; SD_PATH_MAX];
    out[..bytes.len()].copy_from_slice(bytes);
    Some((out, bytes.len() as u8))
}

pub(super) async fn execute_action<'d, SPI>(
    action: FatIoAction,
    probe: &mut SdCardProbe<'d, SPI>,
    engine: &mut FatEngine,
    input: &[u8],
) -> FatIoCompletion
where
    SPI: SdSpiBus,
{
    let result: Result<(), SdProbeError> = match action {
        FatIoAction::ReadSector { lba, .. } => {
            probe
                .read_sector(lba, &mut engine.workspace_mut().sector)
                .await
        }
        FatIoAction::WriteSector { lba, .. } => {
            probe.write_sector(lba, &engine.workspace().sector).await
        }
        // The updater only ever issues FatRequest::Stream (reads land in
        // engine.workspace().sector, driven sector-by-sector) or
        // FatRequest::Write/Append (handled below) — never plain
        // FatRequest::Read, whose ReadSectorToPayload copies into a
        // caller-supplied output buffer this executor doesn't take one of.
        FatIoAction::ReadSectorToPayload { .. } => {
            unreachable!("the updater never issues FatRequest::Read")
        }
        FatIoAction::WriteSectorFromPayload {
            lba,
            payload_offset,
            sector_offset,
            len,
            preserve_existing,
            ..
        } => {
            write_sector_from_payload(
                probe,
                engine,
                input,
                lba,
                payload_offset,
                sector_offset,
                len,
                preserve_existing,
            )
            .await
        }
        FatIoAction::WritePayloadSectors {
            start_lba,
            payload_offset,
            sectors,
            ..
        } => {
            let start = payload_offset as usize;
            let end = start + sectors as usize * sdcard::probe::SD_SECTOR_SIZE;
            probe
                .write_sectors_contiguous(start_lba, &input[start..end])
                .await
        }
    };
    match result {
        Ok(()) => FatIoCompletion::Done,
        Err(err) if err.is_timeout() => FatIoCompletion::TimedOut,
        Err(err) => FatIoCompletion::Failed(err),
    }
}

#[allow(clippy::too_many_arguments)]
async fn write_sector_from_payload<'d, SPI>(
    probe: &mut SdCardProbe<'d, SPI>,
    engine: &mut FatEngine,
    input: &[u8],
    lba: u32,
    payload_offset: u32,
    sector_offset: u16,
    len: u16,
    preserve_existing: bool,
) -> Result<(), SdProbeError>
where
    SPI: SdSpiBus,
{
    if !preserve_existing {
        engine.workspace_mut().sector.fill(0);
    }
    let src = payload_offset as usize;
    let dst = sector_offset as usize;
    let src_end = src + len as usize;
    let dst_end = dst + len as usize;
    engine.workspace_mut().sector[dst..dst_end].copy_from_slice(&input[src..src_end]);
    probe.write_sector(lba, &engine.workspace().sector).await
}
