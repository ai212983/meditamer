use core::cmp;

mod state;

pub(super) use state::{
    AllocationState, DataWriteState, FatWriteState, FreeState, MutationStage, MutationState,
    ZeroWriteState,
};

use super::super::{
    cluster_to_lba, clusters_for_size, encode_short_name, make_short_alias, path_segment_to_name,
    short_name_checksum, DirFound, DirLocation, DirRecord, SdFatError, ATTR_LONG_NAME,
    FAT32_EOC_WRITE, MAX_LFN_SLOTS, SD_SECTOR_SIZE,
};
use super::{
    CommandStage, FatBufferId, FatEngine, FatIoAction, FatReadReturn, FatRequest, FatResult,
    FatStep, FatWriteReturn, ScanReturn,
};

include!("append_upload.rs");
include!("upload_commit.rs");
include!("truncate.rs");
include!("rename_remove.rs");
include!("directory.rs");
include!("chain.rs");
include!("allocation_contiguous.rs");
include!("chain_free.rs");
include!("data_zero.rs");
include!("directory_encode.rs");

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
            let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
            let expected = clusters_for_size(found.record.size as usize, cluster_size);
            self.free.start(
                found.record.first_cluster,
                expected.saturating_add(32) as u32,
            );
            self.mutation.stage = MutationStage::WaitFree;
            self.stage = CommandStage::Free;
            return Ok(FatStep::Continue);
        }
        self.mutation_allocate()
    }
}
