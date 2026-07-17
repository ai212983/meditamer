impl FatEngine {
    fn mutation_allocate(&mut self) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
        let count = clusters_for_size(self.mutation.data_len as usize, cluster_size) as u32;
        self.mutation.target_clusters = count;
        if count == 0 {
            self.mutation.new_first = 0;
            self.mutation.stage = MutationStage::WaitData;
            self.stage = CommandStage::Mutate;
        } else {
            if matches!(self.request, Some(FatRequest::UploadBegin { .. })) {
                self.allocation.start_prefer_contiguous(count, volume);
            } else {
                self.allocation.start(count, volume);
            }
            self.mutation.stage = MutationStage::WaitAllocate;
            self.stage = CommandStage::Allocate;
        }
        Ok(FatStep::Continue)
    }

    fn mutation_write_data(&mut self) -> Result<FatStep, SdFatError> {
        self.mutation.new_first = self.allocation.first;
        if matches!(self.request, Some(FatRequest::UploadBegin { .. })) {
            self.mutation.data_len = 0;
            self.mutation.stage = MutationStage::WaitData;
            self.stage = CommandStage::Mutate;
        } else if self.mutation.data_len == 0 {
            self.mutation.stage = MutationStage::WaitData;
            self.stage = CommandStage::Mutate;
        } else {
            self.data_write.start(
                self.mutation.new_first,
                self.mutation.input,
                self.mutation.data_len,
            );
            self.mutation.stage = MutationStage::WaitData;
            self.stage = CommandStage::DataWrite;
        }
        Ok(FatStep::Continue)
    }

    fn mutation_update_directory(&mut self) -> Result<FatStep, SdFatError> {
        if matches!(self.request, Some(FatRequest::UploadChunk { .. })) {
            self.upload.record.first_cluster = self.mutation.new_first;
            self.upload.record.size = self.mutation.data_len;
            if self.mutation.append_extra > 0 {
                let linked_contiguously = self.mutation.old_clusters == 0
                    || self.allocation.first == self.mutation.tail_cluster.saturating_add(1);
                self.upload.contiguous = self.upload.contiguous
                    && self.allocation.contiguous
                    && linked_contiguously;
                self.upload.allocated_clusters = self.mutation.target_clusters;
                self.upload.tail_cluster = self.allocation.previous;
            }
            return Ok(self.finish(FatResult::Done));
        }
        self.mutation.stage = MutationStage::ReadDirectory;
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }

    pub(super) fn advance_fat_write(&mut self) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        let byte_offset = self.fat_write.cluster as u64 * 4;
        let sector_offset = (byte_offset / SD_SECTOR_SIZE as u64) as u32;
        let index = (byte_offset % SD_SECTOR_SIZE as u64) as usize;
        let fat_base = volume.fat_start_lba.saturating_add(
            (self.fat_write.fat_index as u32).saturating_mul(volume.fat_size_sectors),
        );
        let lba = fat_base.saturating_add(sector_offset);
        match self.fat_write.phase {
            0 => {
                self.fat_write.phase = 1;
                Ok(self.issue(FatIoAction::ReadSector {
                    lba,
                    buffer: FatBufferId::Sector,
                }))
            }
            1 => {
                let old = u32::from_le_bytes([
                    self.workspace.sector[index],
                    self.workspace.sector[index + 1],
                    self.workspace.sector[index + 2],
                    self.workspace.sector[index + 3],
                ]);
                let new = (old & 0xF000_0000) | (self.fat_write.value & 0x0FFF_FFFF);
                self.workspace.sector[index..index + 4].copy_from_slice(&new.to_le_bytes());
                self.fat_write.phase = 2;
                Ok(self.issue(FatIoAction::WriteSector {
                    lba,
                    buffer: FatBufferId::Sector,
                }))
            }
            _ => {
                self.fat_write.fat_index = self.fat_write.fat_index.saturating_add(1);
                if self.fat_write.fat_index < volume.fats {
                    self.fat_write.phase = 0;
                    return Ok(FatStep::Continue);
                }
                self.after_fat_write()
            }
        }
    }

    pub(super) fn advance_allocate(&mut self) -> Result<FatStep, SdFatError> {
        if self.allocation.remaining == 0 {
            self.stage = CommandStage::Mutate;
            return Ok(FatStep::Continue);
        }
        if self.allocation.candidate > self.allocation.max_cluster {
            if self.allocation.prefer_contiguous {
                self.allocation.prefer_contiguous = false;
                self.allocation.candidate = 2;
                self.allocation.phase = 0;
                return Ok(FatStep::Continue);
            }
            return Err(SdFatError::NoFreeCluster);
        }
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        match self.allocation.phase {
            0 => {
                self.allocation.sector_offset = self.allocation.candidate / 128;
                if self.allocation.sector_offset >= volume.fat_size_sectors {
                    return Err(SdFatError::NoFreeCluster);
                }
                self.allocation.phase = 1;
                Ok(self.issue(FatIoAction::ReadSector {
                    lba: volume
                        .fat_start_lba
                        .saturating_add(self.allocation.sector_offset),
                    buffer: FatBufferId::Sector,
                }))
            }
            1 => {
                self.prepare_allocation_batch()?;
                if self.allocation.batch_count == 0 {
                    self.allocation.candidate = self.allocation.batch_next;
                    self.allocation.phase = 0;
                    return Ok(FatStep::Continue);
                }
                self.allocation.fat_index = 0;
                self.allocation.phase = 2;
                Ok(FatStep::Continue)
            }
            _ if self.allocation.fat_index < volume.fats => {
                let fat_index = self.allocation.fat_index;
                self.allocation.fat_index = self.allocation.fat_index.saturating_add(1);
                let lba = volume
                    .fat_start_lba
                    .saturating_add((fat_index as u32).saturating_mul(volume.fat_size_sectors))
                    .saturating_add(self.allocation.sector_offset);
                Ok(self.issue(FatIoAction::WriteSector {
                    lba,
                    buffer: FatBufferId::Sector,
                }))
            }
            _ => self.finish_allocation_batch(),
        }
    }

    fn prepare_allocation_batch(&mut self) -> Result<(), SdFatError> {
        if self.allocation.prefer_contiguous {
            return self.prepare_contiguous_allocation_batch();
        }
        let sector_first = self.allocation.sector_offset.saturating_mul(128);
        let sector_end = sector_first.saturating_add(128);
        let mut candidate = self.allocation.candidate.max(2);
        let mut batch_first = 0;
        let mut batch_last = self.allocation.previous;
        let mut batch_count = 0u32;
        let external_previous = if batch_last >= 2 && batch_last < sector_first {
            batch_last
        } else {
            0
        };

        while candidate < sector_end
            && candidate <= self.allocation.max_cluster
            && batch_count < self.allocation.remaining
        {
            let index = ((candidate - sector_first) * 4) as usize;
            let value = u32::from_le_bytes([
                self.workspace.sector[index],
                self.workspace.sector[index + 1],
                self.workspace.sector[index + 2],
                self.workspace.sector[index + 3],
            ]) & 0x0FFF_FFFF;
            if value == 0 {
                if batch_last >= 2 && candidate != batch_last.saturating_add(1) {
                    self.allocation.contiguous = false;
                }
                if batch_first == 0 {
                    batch_first = candidate;
                }
                if batch_last >= sector_first {
                    write_fat_value(
                        &mut self.workspace.sector,
                        ((batch_last - sector_first) * 4) as usize,
                        candidate,
                    );
                }
                write_fat_value(&mut self.workspace.sector, index, FAT32_EOC_WRITE);
                batch_last = candidate;
                batch_count = batch_count.saturating_add(1);
            }
            candidate = candidate.saturating_add(1);
        }

        self.allocation.batch_first = batch_first;
        self.allocation.batch_last = batch_last;
        self.allocation.batch_count = batch_count;
        self.allocation.batch_next = candidate;
        self.allocation.external_previous = external_previous;
        Ok(())
    }

    fn finish_allocation_batch(&mut self) -> Result<FatStep, SdFatError> {
        if self.allocation.first == 0 {
            self.allocation.first = self.allocation.batch_first;
        }
        self.allocation.previous = self.allocation.batch_last;
        self.allocation.remaining = self
            .allocation
            .remaining
            .saturating_sub(self.allocation.batch_count);
        self.allocation.candidate = self.allocation.batch_next;
        self.allocation.phase = 0;

        if self.allocation.external_previous >= 2 {
            self.fat_write.start(
                self.allocation.external_previous,
                self.allocation.batch_first,
            );
            self.fat_write_return = FatWriteReturn::AllocateBatchLink;
            self.stage = CommandStage::WriteFat;
        } else {
            self.stage = CommandStage::Allocate;
        }
        Ok(FatStep::Continue)
    }

    fn after_fat_write(&mut self) -> Result<FatStep, SdFatError> {
        match self.fat_write_return {
            FatWriteReturn::AllocateBatchLink => {
                self.stage = CommandStage::Allocate;
                Ok(FatStep::Continue)
            }
            FatWriteReturn::DirectoryLink => {
                self.stage = CommandStage::Mutate;
                Ok(FatStep::Continue)
            }
            FatWriteReturn::AppendLink => {
                self.stage = CommandStage::Mutate;
                Ok(FatStep::Continue)
            }
            FatWriteReturn::TruncateLink | FatWriteReturn::TruncateCut => {
                self.stage = CommandStage::Mutate;
                Ok(FatStep::Continue)
            }
        }
    }

}

fn write_fat_value(sector: &mut [u8; SD_SECTOR_SIZE], index: usize, value: u32) {
    let old = u32::from_le_bytes([
        sector[index],
        sector[index + 1],
        sector[index + 2],
        sector[index + 3],
    ]);
    let encoded = (old & 0xF000_0000) | (value & 0x0FFF_FFFF);
    sector[index..index + 4].copy_from_slice(&encoded.to_le_bytes());
}
