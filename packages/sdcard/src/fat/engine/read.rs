use core::cmp;

use super::super::{
    cluster_to_lba, parse_list_dir_slot, Fat32Volume, LfnState, ListDirSlot, SdFatError,
    DIR_ENTRIES_PER_SECTOR,
};
use super::{
    CommandStage, FatBufferId, FatEngine, FatIoAction, FatPayloadId, FatReadReturn, FatResult,
    FatStageLabel, FatStep,
};

pub(super) struct ListState {
    pub(super) cluster: u32,
    pub(super) sector_offset: u8,
    pub(super) visited: u32,
    pub(super) count: usize,
    lfn: LfnState,
    awaiting_sector: bool,
    sector_loaded: bool,
    slot: u8,
}

impl ListState {
    pub(super) fn new() -> Self {
        Self {
            cluster: 0,
            sector_offset: 0,
            visited: 0,
            count: 0,
            lfn: LfnState::new(),
            awaiting_sector: false,
            sector_loaded: false,
            slot: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.cluster = 0;
        self.sector_offset = 0;
        self.visited = 0;
        self.count = 0;
        self.lfn.clear();
        self.awaiting_sector = false;
        self.sector_loaded = false;
        self.slot = 0;
    }

    pub(super) fn start(&mut self, cluster: u32) {
        self.reset();
        self.cluster = cluster;
    }

    pub(super) fn label(&self) -> FatStageLabel {
        FatStageLabel::ListDirectory
    }
}

pub(super) struct ReadState {
    pub(super) cluster: u32,
    pub(super) sector_offset: u8,
    pub(super) visited: u32,
    remaining: u32,
    written: u32,
    output: FatPayloadId,
    awaiting_sector: bool,
}

impl ReadState {
    pub(super) const fn new() -> Self {
        Self {
            cluster: 0,
            sector_offset: 0,
            visited: 0,
            remaining: 0,
            written: 0,
            output: FatPayloadId::Primary,
            awaiting_sector: false,
        }
    }

    pub(super) fn reset(&mut self) {
        self.cluster = 0;
        self.sector_offset = 0;
        self.visited = 0;
        self.remaining = 0;
        self.written = 0;
        self.output = FatPayloadId::Primary;
        self.awaiting_sector = false;
    }

    pub(super) fn start(&mut self, cluster: u32, size: u32, output: FatPayloadId) {
        self.reset();
        self.cluster = cluster;
        self.remaining = size;
        self.output = output;
    }

    pub(super) fn label(&self) -> FatStageLabel {
        FatStageLabel::ReadFile
    }
}

/// Sequential, bounded-memory file read: unlike [`ReadState`], which
/// requires the caller's output buffer to hold the whole file,
/// [`StreamState`] hands back one [`crate::probe::SD_SECTOR_SIZE`]-byte
/// chunk at a time in `workspace.sector`. The caller (factory updater, ADR-
/// 0014 Phase 1) drains each chunk — e.g. into a running SHA-256 digest —
/// between `advance()` calls, so a bundle far larger than any on-device
/// buffer can still be read and hashed.
pub(super) struct StreamState {
    pub(super) cluster: u32,
    pub(super) sector_offset: u8,
    pub(super) visited: u32,
    remaining: u32,
    pub(super) written: u32,
    pub(super) chunk_len: u16,
    awaiting_sector: bool,
}

impl StreamState {
    pub(super) const fn new() -> Self {
        Self {
            cluster: 0,
            sector_offset: 0,
            visited: 0,
            remaining: 0,
            written: 0,
            chunk_len: 0,
            awaiting_sector: false,
        }
    }

    pub(super) fn reset(&mut self) {
        self.cluster = 0;
        self.sector_offset = 0;
        self.visited = 0;
        self.remaining = 0;
        self.written = 0;
        self.chunk_len = 0;
        self.awaiting_sector = false;
    }

    pub(super) fn start(&mut self, cluster: u32, size: u32) {
        self.reset();
        self.cluster = cluster;
        self.remaining = size;
    }

    pub(super) fn label(&self) -> FatStageLabel {
        FatStageLabel::StreamFile
    }
}

impl FatEngine {
    pub(super) fn advance_list(&mut self) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if self.list.visited > volume.total_clusters.saturating_add(2) {
            return Err(SdFatError::ClusterChainTooLong);
        }
        if !self.list.awaiting_sector && !self.list.sector_loaded {
            let lba = cluster_to_lba(&volume, self.list.cluster)?
                .saturating_add(self.list.sector_offset as u32);
            self.list.awaiting_sector = true;
            return Ok(self.issue(FatIoAction::ReadSector {
                lba,
                buffer: FatBufferId::Sector,
            }));
        }
        if self.list.awaiting_sector {
            self.list.awaiting_sector = false;
            self.list.sector_loaded = true;
        }
        let lba = cluster_to_lba(&volume, self.list.cluster)?
            .saturating_add(self.list.sector_offset as u32);
        for slot in self.list.slot as usize..DIR_ENTRIES_PER_SECTOR {
            self.list.slot = slot.saturating_add(1) as u8;
            match parse_list_dir_slot(&self.workspace.sector, lba, slot as u8, &mut self.list.lfn) {
                ListDirSlot::Continue => {}
                ListDirSlot::End => {
                    return Ok(self.finish(FatResult::Listed {
                        count: self.list.count as u8,
                    }));
                }
                ListDirSlot::Entry(entry) => {
                    if self.list.count >= super::FAT_ENGINE_LIST_CAPACITY {
                        return Ok(self.finish(FatResult::Listed {
                            count: self.list.count as u8,
                        }));
                    }
                    self.workspace.entry = entry;
                    self.list.count += 1;
                    return Ok(FatStep::Continue);
                }
            }
        }
        self.list.slot = 0;
        self.list.sector_loaded = false;
        self.list.sector_offset = self.list.sector_offset.saturating_add(1);
        if self.list.sector_offset < volume.sectors_per_cluster {
            return Ok(FatStep::Continue);
        }
        self.fat_read.start(self.list.cluster);
        self.fat_read_return = FatReadReturn::List;
        self.stage = CommandStage::ReadFat;
        Ok(FatStep::Continue)
    }

    pub(super) fn advance_read(&mut self) -> Result<FatStep, SdFatError> {
        let volume: Fat32Volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if self.read.remaining == 0 {
            return Ok(self.finish(FatResult::Read {
                bytes: self.read.written,
            }));
        }
        if self.read.visited > volume.total_clusters.saturating_add(2) {
            return Err(SdFatError::ClusterChainTooLong);
        }
        if !self.read.awaiting_sector {
            let lba = cluster_to_lba(&volume, self.read.cluster)?
                .saturating_add(self.read.sector_offset as u32);
            let len = cmp::min(self.read.remaining as usize, crate::probe::SD_SECTOR_SIZE) as u16;
            self.read.awaiting_sector = true;
            return Ok(self.issue(FatIoAction::ReadSectorToPayload {
                lba,
                buffer: FatBufferId::Sector,
                payload: self.read.output,
                payload_offset: self.read.written,
                len,
            }));
        }
        self.read.awaiting_sector = false;
        let consumed = cmp::min(self.read.remaining, crate::probe::SD_SECTOR_SIZE as u32);
        self.read.remaining -= consumed;
        self.read.written = self.read.written.saturating_add(consumed);
        self.read.sector_offset = self.read.sector_offset.saturating_add(1);
        if self.read.remaining == 0 {
            return Ok(FatStep::Continue);
        }
        if self.read.sector_offset < volume.sectors_per_cluster {
            return Ok(FatStep::Continue);
        }
        self.fat_read.start(self.read.cluster);
        self.fat_read_return = FatReadReturn::Read;
        self.stage = CommandStage::ReadFat;
        Ok(FatStep::Continue)
    }

    /// Advances one [`StreamState`] step. On every `Continue` returned after
    /// the initial `Io` round-trip, `workspace.sector[..stream_chunk_len()]`
    /// holds one freshly read, still-unconsumed chunk — read it before
    /// calling `advance` again, which starts filling `sector` with the next
    /// chunk (or, at a cluster boundary, first walks the FAT chain).
    pub(super) fn advance_stream(&mut self) -> Result<FatStep, SdFatError> {
        let volume: Fat32Volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if self.stream.remaining == 0 {
            return Ok(self.finish(FatResult::Streamed {
                bytes: self.stream.written,
            }));
        }
        if self.stream.visited > volume.total_clusters.saturating_add(2) {
            return Err(SdFatError::ClusterChainTooLong);
        }
        if !self.stream.awaiting_sector {
            let lba = cluster_to_lba(&volume, self.stream.cluster)?
                .saturating_add(self.stream.sector_offset as u32);
            self.stream.awaiting_sector = true;
            return Ok(self.issue(FatIoAction::ReadSector {
                lba,
                buffer: FatBufferId::Sector,
            }));
        }
        self.stream.awaiting_sector = false;
        let consumed = cmp::min(self.stream.remaining, crate::probe::SD_SECTOR_SIZE as u32);
        self.stream.chunk_len = consumed as u16;
        self.stream.remaining -= consumed;
        self.stream.written = self.stream.written.saturating_add(consumed);
        self.stream.sector_offset = self.stream.sector_offset.saturating_add(1);
        if self.stream.remaining == 0 {
            return Ok(FatStep::Continue);
        }
        if self.stream.sector_offset < volume.sectors_per_cluster {
            return Ok(FatStep::Continue);
        }
        self.fat_read.start(self.stream.cluster);
        self.fat_read_return = FatReadReturn::Stream;
        self.stage = CommandStage::ReadFat;
        Ok(FatStep::Continue)
    }
}
