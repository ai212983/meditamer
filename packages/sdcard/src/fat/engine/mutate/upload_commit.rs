impl FatEngine {
    fn begin_upload_commit(&mut self) -> Result<FatStep, SdFatError> {
        if !self.upload.valid {
            return Ok(self.finish(FatResult::Error(super::FatEngineError::InvalidState)));
        }
        self.mutation.new_first = self.upload.record.first_cluster;
        self.mutation.data_len = self.upload.record.size;
        self.mutation.stage = MutationStage::WaitData;
        self.mutation_update_directory()
    }

    fn begin_cached_upload_rename(&mut self) -> Result<FatStep, SdFatError> {
        let source = self.target.ok_or(SdFatError::NotFound)?;
        let source_parent = self.upload.parent_cluster;
        let (dst_path, dst_len) = self.rename_destination()?;
        self.mutation.reset();
        self.mutation.rename_source = Some(source);
        self.mutation.rename_source_parent = source_parent;
        self.prepare_rename_destination(dst_path, dst_len)
    }

    fn rename_destination(
        &self,
    ) -> Result<([u8; super::super::super::SD_PATH_MAX], u8), SdFatError> {
        match self.request.as_ref() {
            Some(FatRequest::Rename {
                dst_path,
                dst_path_len,
                ..
            }) => Ok((*dst_path, *dst_path_len)),
            Some(FatRequest::UploadCommit { path, path_len }) => Ok((*path, *path_len)),
            _ => Err(SdFatError::InvalidPath),
        }
    }

    fn rename_replace(&self) -> bool {
        match self.request.as_ref() {
            Some(FatRequest::Rename { replace, .. }) => *replace,
            Some(FatRequest::UploadCommit { .. }) => true,
            _ => false,
        }
    }

    fn is_rename_operation(&self) -> bool {
        matches!(
            self.request,
            Some(FatRequest::Rename { .. } | FatRequest::UploadCommit { .. })
        )
    }

    fn finish_rename_operation(&mut self) -> FatStep {
        if matches!(self.request, Some(FatRequest::UploadCommit { .. })) {
            self.upload.clear();
        }
        self.finish(FatResult::Done)
    }

    fn begin_upload_commit_replace(&mut self) -> Result<FatStep, SdFatError> {
        self.mutation.directory_sector_pending = false;
        self.mutation.stage = MutationStage::UploadCommitReplace;
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }

    fn advance_upload_commit_replace(&mut self) -> Result<FatStep, SdFatError> {
        let destination = self.target.ok_or(SdFatError::NotFound)?;
        let source = self.mutation.rename_source.ok_or(SdFatError::NotFound)?;
        if !self.mutation.directory_sector_pending {
            self.mutation.directory_sector_pending = true;
            return Ok(self.issue(FatIoAction::ReadSector {
                lba: destination.short_location.lba,
                buffer: FatBufferId::Sector,
            }));
        }

        let mut record = destination.record;
        record.attr = source.record.attr;
        record.first_cluster = source.record.first_cluster;
        record.size = source.record.size;
        write_record_to_sector(
            &mut self.workspace.sector,
            destination.short_location,
            record,
        );
        self.target = Some(source);
        self.mutation.delete_index = 0;
        self.mutation.delete_return = 2;
        self.mutation.directory_sector_pending = false;
        self.mutation.stage = MutationStage::RenameDeleteSource;
        Ok(self.issue(FatIoAction::WriteSector {
            lba: destination.short_location.lba,
            buffer: FatBufferId::Sector,
        }))
    }
}
