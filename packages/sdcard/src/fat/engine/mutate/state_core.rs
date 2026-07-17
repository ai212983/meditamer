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
