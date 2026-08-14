//! Mutating FAT commands.
//!
//! Each submodule owns one family of mutation -- appending and uploading,
//! truncating, renaming and removing, directory growth, and the cluster-chain
//! and data-write stages they all share. This module holds the stage dispatcher
//! and the shared mutation state.

mod allocation_contiguous;
mod append_upload;
mod chain;
mod chain_free;
mod data_zero;
mod directory;
mod directory_encode;
mod rename_remove;
mod state;
mod truncate;
mod upload_commit;

pub(super) use state::{
    AllocationState, DataWriteState, FatWriteState, FreeState, MutationStage, MutationState,
    ZeroWriteState,
};

use super::super::{DirFound, SdFatError};
use super::{CommandStage, FatEngine, FatRequest, FatResult, FatStep};

impl FatEngine {
    pub(super) fn begin_mutation(
        &mut self,
        found: Option<DirFound>,
    ) -> Result<FatStep, SdFatError> {
        self.mutation.reset();
        self.target = found;
        self.mutation.parent_cluster = self.resolve.cluster();
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }

    pub(super) fn advance_mutation(&mut self) -> Result<FatStep, SdFatError> {
        match self.mutation.stage {
            MutationStage::Start => self.mutation_start(),
            MutationStage::WaitAliasScan | MutationStage::WaitFreeScan => {
                Err(SdFatError::InvalidPath)
            }
            MutationStage::WaitDirectoryAllocate => self.link_extended_directory(),
            MutationStage::WaitDirectoryLink | MutationStage::ZeroDirectoryCluster => {
                self.zero_extended_directory()
            }
            MutationStage::WaitMkdirAllocate | MutationStage::InitializeMkdir => {
                self.initialize_mkdir()
            }
            MutationStage::WaitEmptyScan => Err(SdFatError::InvalidPath),
            MutationStage::WaitRemoveFree => self.begin_delete_entry(),
            MutationStage::DeleteEntry => self.advance_delete_entry(),
            MutationStage::AppendTraverse => self.advance_append_traverse(),
            MutationStage::AppendWaitAllocate => self.finish_append_allocation(),
            MutationStage::AppendWaitLink => self.start_append_data(),
            MutationStage::TruncateTraverse => self.advance_truncate_traverse(),
            MutationStage::TruncateWaitAllocate => self.finish_truncate_allocation(),
            MutationStage::TruncateWaitLink => self.start_truncate_zero(),
            MutationStage::TruncateReadFreeStart => self.read_truncate_free_start(),
            MutationStage::TruncateWaitCut => self.free_truncated_suffix(),
            MutationStage::TruncateWaitFree => self.start_truncate_zero(),
            MutationStage::TruncateZero => self.finish_truncate(),
            MutationStage::RenameResolve | MutationStage::RenameTarget => {
                Err(SdFatError::InvalidPath)
            }
            MutationStage::RenameWaitDestFree => self.begin_rename_replacement(),
            MutationStage::RenameDeleteDest | MutationStage::RenameDeleteSource => {
                self.advance_delete_entry()
            }
            MutationStage::UploadCommitReplace => self.advance_upload_commit_replace(),
            MutationStage::WaitFree => self.mutation_allocate(),
            MutationStage::WaitAllocate => self.mutation_write_data(),
            MutationStage::WaitData => self.mutation_update_directory(),
            MutationStage::ReadDirectory => self.mutation_read_directory(),
            MutationStage::WriteDirectory => self.mutation_write_directory_done(),
        }
    }

    fn mutation_start(&mut self) -> Result<FatStep, SdFatError> {
        if matches!(self.request, Some(FatRequest::UploadClear)) {
            return Ok(self.finish(FatResult::Done));
        }
        if matches!(self.request, Some(FatRequest::UploadFlush)) {
            if !self.upload.valid {
                return Ok(self.finish(FatResult::Error(super::FatEngineError::InvalidState)));
            }
            self.mutation.new_first = self.upload.record.first_cluster;
            self.mutation.data_len = self.upload.record.size;
            self.mutation.stage = MutationStage::WaitData;
            return self.mutation_update_directory();
        }
        if matches!(self.request, Some(FatRequest::UploadCommit { .. })) {
            return self.begin_upload_commit();
        }
        if matches!(self.request, Some(FatRequest::UploadChunk { .. })) {
            return self.begin_upload_chunk();
        }
        if matches!(self.request, Some(FatRequest::Truncate { .. })) {
            return self.begin_truncate();
        }
        if matches!(self.request, Some(FatRequest::Append { .. })) {
            return self.begin_append();
        }
        if matches!(self.request, Some(FatRequest::Remove { .. })) {
            return self.begin_remove();
        }
        if matches!(self.request, Some(FatRequest::Mkdir { .. })) {
            if self.target.is_some() {
                return Err(SdFatError::AlreadyExists);
            }
            return self.begin_new_file_name();
        }
        let (input, len) = match self.request.as_ref() {
            Some(FatRequest::Write {
                input, input_len, ..
            }) => (*input, *input_len),
            Some(FatRequest::UploadBegin { expected_size, .. }) => {
                (super::FatPayloadId::Primary, *expected_size)
            }
            _ => {
                return Ok(self.finish(FatResult::Error(super::FatEngineError::UnsupportedRequest)))
            }
        };
        let found = match self.target {
            Some(found) => found,
            None => return self.begin_new_file_name(),
        };
        if found.record.is_dir() {
            return Err(SdFatError::IsDirectory);
        }
        self.mutation.input = input;
        self.mutation.data_len = len;
        if found.record.first_cluster >= 2 {
            let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
            // See the comment in `free_remove_chain_or_delete`
            // (rename_remove.rs): the existing entry's on-disk `size` cannot
            // be trusted as a chain-length estimate here either -- this same
            // branch runs when `UploadBegin` retargets a path that already
            // has an allocated chain (e.g. re-attempting an upload after a
            // prior attempt was aborted mid-write), and that prior chain can
            // be far longer than its never-persisted `size` field implies.
            // Bound by the volume's total cluster count instead.
            self.free.start(
                found.record.first_cluster,
                volume.total_clusters.saturating_add(2),
            );
            self.mutation.stage = MutationStage::WaitFree;
            self.stage = CommandStage::Free;
            return Ok(FatStep::Continue);
        }
        self.mutation_allocate()
    }
}
