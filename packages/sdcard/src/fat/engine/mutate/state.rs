//! Per-stage mutation state.
//!
//! Plain data holders for the mutation command and the block-device stages it
//! drives. They carry no logic beyond reset and cursor bookkeeping.

use super::super::super::{DirFound, Fat32Volume};
use super::super::{FatPayloadId, FatStageLabel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MutationStage {
    Start,
    WaitAliasScan,
    WaitFreeScan,
    WaitDirectoryAllocate,
    WaitDirectoryLink,
    ZeroDirectoryCluster,
    WaitMkdirAllocate,
    InitializeMkdir,
    WaitEmptyScan,
    WaitRemoveFree,
    DeleteEntry,
    AppendTraverse,
    AppendWaitAllocate,
    AppendWaitLink,
    TruncateTraverse,
    TruncateWaitAllocate,
    TruncateWaitLink,
    TruncateReadFreeStart,
    TruncateWaitCut,
    TruncateWaitFree,
    TruncateZero,
    RenameResolve,
    RenameTarget,
    RenameWaitDestFree,
    RenameDeleteDest,
    RenameDeleteSource,
    UploadCommitReplace,
    WaitFree,
    WaitAllocate,
    WaitData,
    ReadDirectory,
    WriteDirectory,
}

pub(crate) struct MutationState {
    pub(crate) stage: MutationStage,
    pub(super) new_first: u32,
    pub(super) data_len: u32,
    pub(super) input: FatPayloadId,
    pub(super) directory_sector_pending: bool,
    pub(crate) parent_cluster: u32,
    pub(super) short_name: [u8; 11],
    pub(super) alias_attempt: u32,
    pub(super) lfn_len: u16,
    pub(super) needed_slots: u8,
    pub(super) new_entry: bool,
    pub(super) entry_index: u8,
    pub(super) directory_tail: u32,
    pub(super) directory_new_cluster: u32,
    pub(super) zero_sector: u8,
    pub(super) delete_index: u8,
    pub(super) old_size: u32,
    pub(super) old_first: u32,
    pub(super) tail_cluster: u32,
    pub(super) traverse_remaining: u32,
    pub(super) append_extra: u32,
    pub(super) target_size: u32,
    pub(super) old_clusters: u32,
    pub(super) target_clusters: u32,
    pub(super) free_start: u32,
    pub(super) rename_source: Option<DirFound>,
    pub(super) rename_source_parent: u32,
    pub(super) delete_return: u8,
}

impl MutationState {
    pub(crate) const fn new() -> Self {
        Self {
            stage: MutationStage::Start,
            new_first: 0,
            data_len: 0,
            input: FatPayloadId::Primary,
            directory_sector_pending: false,
            parent_cluster: 0,
            short_name: [0; 11],
            alias_attempt: 1,
            lfn_len: 0,
            needed_slots: 0,
            new_entry: false,
            entry_index: 0,
            directory_tail: 0,
            directory_new_cluster: 0,
            zero_sector: 0,
            delete_index: 0,
            old_size: 0,
            old_first: 0,
            tail_cluster: 0,
            traverse_remaining: 0,
            append_extra: 0,
            target_size: 0,
            old_clusters: 0,
            target_clusters: 0,
            free_start: 0,
            rename_source: None,
            rename_source_parent: 0,
            delete_return: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.stage = MutationStage::Start;
        self.new_first = 0;
        self.data_len = 0;
        self.input = FatPayloadId::Primary;
        self.directory_sector_pending = false;
        self.parent_cluster = 0;
        self.short_name = [0; 11];
        self.alias_attempt = 1;
        self.lfn_len = 0;
        self.needed_slots = 0;
        self.new_entry = false;
        self.entry_index = 0;
        self.directory_tail = 0;
        self.directory_new_cluster = 0;
        self.zero_sector = 0;
        self.delete_index = 0;
        self.old_size = 0;
        self.old_first = 0;
        self.tail_cluster = 0;
        self.traverse_remaining = 0;
        self.append_extra = 0;
        self.target_size = 0;
        self.old_clusters = 0;
        self.target_clusters = 0;
        self.free_start = 0;
        self.rename_source = None;
        self.rename_source_parent = 0;
        self.delete_return = 0;
    }

    pub(crate) fn label(&self) -> FatStageLabel {
        match self.stage {
            MutationStage::ReadDirectory | MutationStage::WriteDirectory => {
                FatStageLabel::UpdateDirectory
            }
            _ => FatStageLabel::WriteFile,
        }
    }

    pub(crate) fn is_rename_resolve(&self) -> bool {
        self.stage == MutationStage::RenameResolve
    }
}

pub(crate) struct ZeroWriteState {
    pub(super) cluster: u32,
    pub(super) sector_offset: u8,
    pub(super) byte_offset: u16,
    pub(super) remaining: u32,
    pub(super) phase: u8,
    pub(super) action_len: u16,
}

impl ZeroWriteState {
    pub(crate) const fn new() -> Self {
        Self {
            cluster: 0,
            sector_offset: 0,
            byte_offset: 0,
            remaining: 0,
            phase: 0,
            action_len: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.cluster = 0;
        self.sector_offset = 0;
        self.byte_offset = 0;
        self.remaining = 0;
        self.phase = 0;
        self.action_len = 0;
    }

    pub(super) fn start(&mut self, cluster: u32, sector_offset: u8, byte_offset: u16, len: u32) {
        self.reset();
        self.cluster = cluster;
        self.sector_offset = sector_offset;
        self.byte_offset = byte_offset;
        self.remaining = len;
    }
}

pub(crate) struct FatWriteState {
    pub(super) cluster: u32,
    pub(super) value: u32,
    pub(super) fat_index: u8,
    pub(super) phase: u8,
}

impl FatWriteState {
    pub(crate) const fn new() -> Self {
        Self {
            cluster: 0,
            value: 0,
            fat_index: 0,
            phase: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.cluster = 0;
        self.value = 0;
        self.fat_index = 0;
        self.phase = 0;
    }

    pub(super) fn start(&mut self, cluster: u32, value: u32) {
        self.cluster = cluster;
        self.value = value;
        self.fat_index = 0;
        self.phase = 0;
    }
}

pub(crate) struct AllocationState {
    pub(super) remaining: u32,
    pub(super) candidate: u32,
    pub(super) max_cluster: u32,
    pub(super) first: u32,
    pub(super) previous: u32,
    pub(super) phase: u8,
    pub(super) sector_offset: u32,
    pub(super) fat_index: u8,
    pub(super) batch_first: u32,
    pub(super) batch_last: u32,
    pub(super) batch_count: u32,
    pub(super) batch_next: u32,
    pub(super) external_previous: u32,
    pub(super) contiguous: bool,
    pub(super) prefer_contiguous: bool,
}

impl AllocationState {
    pub(crate) const fn new() -> Self {
        Self {
            remaining: 0,
            candidate: 2,
            max_cluster: 0,
            first: 0,
            previous: 0,
            phase: 0,
            sector_offset: 0,
            fat_index: 0,
            batch_first: 0,
            batch_last: 0,
            batch_count: 0,
            batch_next: 2,
            external_previous: 0,
            contiguous: true,
            prefer_contiguous: false,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.remaining = 0;
        self.candidate = 2;
        self.max_cluster = 0;
        self.first = 0;
        self.previous = 0;
        self.phase = 0;
        self.sector_offset = 0;
        self.fat_index = 0;
        self.batch_first = 0;
        self.batch_last = 0;
        self.batch_count = 0;
        self.batch_next = 2;
        self.external_previous = 0;
        self.contiguous = true;
        self.prefer_contiguous = false;
    }

    pub(super) fn start(&mut self, count: u32, volume: Fat32Volume) {
        self.reset();
        self.remaining = count;
        self.max_cluster = volume.total_clusters.saturating_add(1);
    }

    pub(super) fn start_prefer_contiguous(&mut self, count: u32, volume: Fat32Volume) {
        self.start(count, volume);
        self.prefer_contiguous = count > 1 && count <= 128;
    }
}

pub(crate) struct FreeState {
    pub(super) current: u32,
    pub(super) steps: u32,
    pub(super) max_steps: u32,
    pub(super) phase: u8,
    pub(super) sector_offset: u32,
    pub(super) fat_index: u8,
    pub(super) batch_next: u32,
    pub(super) batch_count: u32,
}

impl FreeState {
    pub(crate) const fn new() -> Self {
        Self {
            current: 0,
            steps: 0,
            max_steps: 0,
            phase: 0,
            sector_offset: 0,
            fat_index: 0,
            batch_next: 0,
            batch_count: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.current = 0;
        self.steps = 0;
        self.max_steps = 0;
        self.phase = 0;
        self.sector_offset = 0;
        self.fat_index = 0;
        self.batch_next = 0;
        self.batch_count = 0;
    }

    pub(super) fn start(&mut self, first: u32, max_steps: u32) {
        self.current = first;
        self.steps = 0;
        self.max_steps = max_steps.max(1);
        self.phase = 0;
    }
}

pub(crate) struct DataWriteState {
    pub(super) cluster: u32,
    pub(super) sector_offset: u8,
    pub(super) payload: FatPayloadId,
    pub(super) payload_offset: u32,
    pub(super) remaining: u32,
    pub(super) phase: u8,
    pub(super) action_len: u16,
    pub(super) action_sectors: u16,
    pub(super) sector_byte_offset: u16,
}

impl DataWriteState {
    pub(crate) const fn new() -> Self {
        Self {
            cluster: 0,
            sector_offset: 0,
            payload: FatPayloadId::Primary,
            payload_offset: 0,
            remaining: 0,
            phase: 0,
            action_len: 0,
            action_sectors: 0,
            sector_byte_offset: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.cluster = 0;
        self.sector_offset = 0;
        self.payload = FatPayloadId::Primary;
        self.payload_offset = 0;
        self.remaining = 0;
        self.phase = 0;
        self.action_len = 0;
        self.action_sectors = 0;
        self.sector_byte_offset = 0;
    }

    pub(super) fn start(&mut self, cluster: u32, payload: FatPayloadId, len: u32) {
        self.reset();
        self.cluster = cluster;
        self.payload = payload;
        self.remaining = len;
    }

    pub(super) fn start_at(
        &mut self,
        cluster: u32,
        sector_offset: u8,
        sector_byte_offset: u16,
        payload: FatPayloadId,
        len: u32,
    ) {
        self.start(cluster, payload, len);
        self.sector_offset = sector_offset;
        self.sector_byte_offset = sector_byte_offset;
    }
}
