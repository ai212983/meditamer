impl FatEngine {
    pub(super) fn advance_free(&mut self) -> Result<FatStep, SdFatError> {
        if self.free.current < 2 {
            self.stage = CommandStage::Mutate;
            return Ok(FatStep::Continue);
        }
        if self.free.steps >= self.free.max_steps {
            return Err(SdFatError::ClusterChainTooLong);
        }
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        match self.free.phase {
            0 => {
                self.free.sector_offset = self.free.current / 128;
                if self.free.sector_offset >= volume.fat_size_sectors {
                    return Err(SdFatError::BadCluster(self.free.current));
                }
                self.free.phase = 1;
                Ok(self.issue(FatIoAction::ReadSector {
                    lba: volume.fat_start_lba.saturating_add(self.free.sector_offset),
                    buffer: FatBufferId::Sector,
                }))
            }
            1 => {
                self.prepare_free_batch()?;
                self.free.fat_index = 0;
                self.free.phase = 2;
                Ok(FatStep::Continue)
            }
            _ if self.free.fat_index < volume.fats => {
                let fat_index = self.free.fat_index;
                self.free.fat_index = self.free.fat_index.saturating_add(1);
                let lba = volume
                    .fat_start_lba
                    .saturating_add((fat_index as u32).saturating_mul(volume.fat_size_sectors))
                    .saturating_add(self.free.sector_offset);
                Ok(self.issue(FatIoAction::WriteSector {
                    lba,
                    buffer: FatBufferId::Sector,
                }))
            }
            _ => {
                self.free.current = self.free.batch_next;
                self.free.steps = self.free.steps.saturating_add(self.free.batch_count);
                self.free.phase = 0;
                self.stage = CommandStage::Free;
                Ok(FatStep::Continue)
            }
        }
    }

    fn prepare_free_batch(&mut self) -> Result<(), SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        let sector_first = self.free.sector_offset.saturating_mul(128);
        let sector_end = sector_first.saturating_add(128);
        let mut current = self.free.current;
        let mut count = 0u32;

        loop {
            if current < sector_first || current >= sector_end {
                break;
            }
            let index = ((current - sector_first) * 4) as usize;
            let next = u32::from_le_bytes([
                self.workspace.sector[index],
                self.workspace.sector[index + 1],
                self.workspace.sector[index + 2],
                self.workspace.sector[index + 3],
            ]) & 0x0FFF_FFFF;
            write_fat_value(&mut self.workspace.sector, index, 0);
            count = count.saturating_add(1);

            if next >= super::super::FAT32_EOC || next < 2 {
                current = 0;
                break;
            }
            if next > volume.total_clusters.saturating_add(1) {
                return Err(SdFatError::BadCluster(next));
            }
            if self.free.steps.saturating_add(count) >= self.free.max_steps {
                return Err(SdFatError::ClusterChainTooLong);
            }
            current = next;
        }

        self.free.batch_next = current;
        self.free.batch_count = count;
        Ok(())
    }
}
