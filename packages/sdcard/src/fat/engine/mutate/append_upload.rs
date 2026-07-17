impl FatEngine {
    fn begin_upload_chunk(&mut self) -> Result<FatStep, SdFatError> {
        if !self.upload.valid {
            return Ok(self.finish(FatResult::Error(super::FatEngineError::InvalidState)));
        }
        let (input, input_len) = match self.request.as_ref() {
            Some(FatRequest::UploadChunk {
                input, input_len, ..
            }) => (*input, *input_len),
            _ => return Err(SdFatError::InvalidPath),
        };
        if input_len == 0 {
            return Ok(self.finish(FatResult::Done));
        }
        let new_size = self
            .upload
            .record
            .size
            .checked_add(input_len)
            .ok_or(SdFatError::BufferTooSmall { needed: usize::MAX })?;
        let volume = self.upload.volume;
        let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
        let target_clusters = clusters_for_size(new_size as usize, cluster_size) as u32;
        self.mutation.input = input;
        self.mutation.old_size = self.upload.record.size;
        self.mutation.data_len = new_size;
        self.mutation.old_first = self.upload.record.first_cluster;
        self.mutation.new_first = self.upload.record.first_cluster;
        self.mutation.tail_cluster = if self.upload.write_cluster >= 2 {
            self.upload.write_cluster
        } else {
            self.upload.tail_cluster
        };
        self.mutation.old_clusters = self.upload.allocated_clusters;
        self.mutation.target_clusters = target_clusters;
        self.mutation.append_extra = target_clusters.saturating_sub(self.upload.allocated_clusters);
        if self.mutation.append_extra == 0 {
            return self.start_append_data();
        }
        self.allocation.start(self.mutation.append_extra, volume);
        self.mutation.stage = MutationStage::AppendWaitAllocate;
        self.stage = CommandStage::Allocate;
        Ok(FatStep::Continue)
    }

    fn begin_append(&mut self) -> Result<FatStep, SdFatError> {
        let found = self.target.ok_or(SdFatError::NotFound)?;
        if found.record.is_dir() {
            return Err(SdFatError::IsDirectory);
        }
        let (input, input_len) = match self.request.as_ref() {
            Some(FatRequest::Append {
                input, input_len, ..
            }) => (*input, *input_len),
            _ => return Err(SdFatError::InvalidPath),
        };
        if input_len == 0 {
            return Ok(self.finish(FatResult::Done));
        }
        let new_size = found
            .record
            .size
            .checked_add(input_len)
            .ok_or(SdFatError::BufferTooSmall { needed: usize::MAX })?;
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
        let old_clusters = clusters_for_size(found.record.size as usize, cluster_size) as u32;
        let new_clusters = clusters_for_size(new_size as usize, cluster_size) as u32;
        self.mutation.input = input;
        self.mutation.data_len = new_size;
        self.mutation.old_size = found.record.size;
        self.mutation.old_first = found.record.first_cluster;
        self.mutation.new_first = found.record.first_cluster;
        self.mutation.append_extra = new_clusters.saturating_sub(old_clusters);

        if old_clusters == 0 {
            self.allocation.start(new_clusters, volume);
            self.mutation.stage = MutationStage::AppendWaitAllocate;
            self.stage = CommandStage::Allocate;
            return Ok(FatStep::Continue);
        }
        if found.record.first_cluster < 2 {
            return Err(SdFatError::BadCluster(found.record.first_cluster));
        }
        self.mutation.tail_cluster = found.record.first_cluster;
        self.mutation.traverse_remaining = old_clusters.saturating_sub(1);
        self.mutation.stage = MutationStage::AppendTraverse;
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }

    fn advance_append_traverse(&mut self) -> Result<FatStep, SdFatError> {
        if self.mutation.traverse_remaining == 0 {
            if self.mutation.append_extra == 0 {
                return self.start_append_data();
            }
            let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
            self.allocation.start(self.mutation.append_extra, volume);
            self.mutation.stage = MutationStage::AppendWaitAllocate;
            self.stage = CommandStage::Allocate;
            return Ok(FatStep::Continue);
        }
        self.fat_read.start(self.mutation.tail_cluster);
        self.fat_read_return = FatReadReturn::AppendTraverse;
        self.stage = CommandStage::ReadFat;
        Ok(FatStep::Continue)
    }

    fn finish_append_allocation(&mut self) -> Result<FatStep, SdFatError> {
        if self.mutation.old_first < 2 {
            self.mutation.new_first = self.allocation.first;
            self.mutation.tail_cluster = self.allocation.first;
            return self.start_append_data();
        }
        self.fat_write
            .start(self.mutation.tail_cluster, self.allocation.first);
        self.fat_write_return = FatWriteReturn::AppendLink;
        self.mutation.stage = MutationStage::AppendWaitLink;
        self.stage = CommandStage::WriteFat;
        Ok(FatStep::Continue)
    }

    fn start_append_data(&mut self) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        let cluster_size = SD_SECTOR_SIZE as u32 * u32::from(volume.sectors_per_cluster);
        let offset_in_cluster = self.mutation.old_size % cluster_size;
        let cluster = if offset_in_cluster == 0
            && self.mutation.old_size != 0
            && self.mutation.append_extra > 0
        {
            self.allocation.first
        } else {
            self.mutation.tail_cluster
        };
        self.data_write.start_at(
            cluster,
            (offset_in_cluster / SD_SECTOR_SIZE as u32) as u8,
            (offset_in_cluster % SD_SECTOR_SIZE as u32) as u16,
            self.mutation.input,
            self.mutation.data_len - self.mutation.old_size,
        );
        self.mutation.stage = MutationStage::WaitData;
        self.stage = CommandStage::DataWrite;
        Ok(FatStep::Continue)
    }
}
