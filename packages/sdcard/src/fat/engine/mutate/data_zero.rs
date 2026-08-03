impl FatEngine {
    pub(super) fn advance_data_write(&mut self) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if self.data_write.remaining == 0 {
            if matches!(self.request, Some(FatRequest::UploadChunk { .. })) {
                if self.upload.contiguous {
                    self.upload.write_cluster = if self.data_write.sector_offset == 0
                        && self.mutation.target_clusters >= self.upload.allocated_clusters
                    {
                        self.data_write.cluster.saturating_sub(1)
                    } else {
                        self.data_write.cluster
                    };
                    self.stage = CommandStage::Mutate;
                    return Ok(FatStep::Continue);
                }
                let cluster_size =
                    SD_SECTOR_SIZE as u32 * u32::from(volume.sectors_per_cluster);
                if self.mutation.data_len != 0
                    && self.mutation.data_len.is_multiple_of(cluster_size)
                    && self.mutation.target_clusters < self.upload.allocated_clusters
                {
                    self.fat_read.start(self.data_write.cluster);
                    self.fat_read_return = FatReadReturn::UploadCursor;
                    self.stage = CommandStage::ReadFat;
                    return Ok(FatStep::Continue);
                }
                self.upload.write_cluster = self.data_write.cluster;
            }
            self.stage = CommandStage::Mutate;
            return Ok(FatStep::Continue);
        }
        if self.data_write.phase == 0 {
            let full = self.data_write.remaining / SD_SECTOR_SIZE as u32;
            let sectors_left = if matches!(self.request, Some(FatRequest::UploadChunk { .. }))
                && self.upload.contiguous
            {
                full
            } else {
                (volume.sectors_per_cluster - self.data_write.sector_offset) as u32
            };
            let burst = if self.data_write.sector_byte_offset == 0 {
                cmp::min(sectors_left, full).min(u16::MAX as u32) as u16
            } else {
                0
            };
            let lba = cluster_to_lba(&volume, self.data_write.cluster)?
                .saturating_add(self.data_write.sector_offset as u32);
            if burst >= 2 {
                self.data_write.phase = 2;
                self.data_write.action_sectors = burst;
                return Ok(self.issue(FatIoAction::WritePayloadSectors {
                    start_lba: lba,
                    payload: self.data_write.payload,
                    payload_offset: self.data_write.payload_offset,
                    sectors: burst,
                }));
            }
            let room = SD_SECTOR_SIZE as u16 - self.data_write.sector_byte_offset;
            let len = cmp::min(self.data_write.remaining, u32::from(room)) as u16;
            self.data_write.action_len = len;
            self.data_write.action_sectors = 1;
            if self.data_write.sector_byte_offset != 0 || len as usize != SD_SECTOR_SIZE {
                self.data_write.phase = 1;
                return Ok(self.issue(FatIoAction::ReadSector {
                    lba,
                    buffer: FatBufferId::Sector,
                }));
            }
            self.data_write.phase = 2;
            return Ok(self.issue(FatIoAction::WriteSectorFromPayload {
                lba,
                buffer: FatBufferId::Sector,
                payload: self.data_write.payload,
                payload_offset: self.data_write.payload_offset,
                sector_offset: self.data_write.sector_byte_offset,
                len,
                preserve_existing: false,
            }));
        }
        if self.data_write.phase == 1 {
            let lba = cluster_to_lba(&volume, self.data_write.cluster)?
                .saturating_add(self.data_write.sector_offset as u32);
            self.data_write.phase = 2;
            return Ok(self.issue(FatIoAction::WriteSectorFromPayload {
                lba,
                buffer: FatBufferId::Sector,
                payload: self.data_write.payload,
                payload_offset: self.data_write.payload_offset,
                sector_offset: self.data_write.sector_byte_offset,
                len: self.data_write.action_len,
                preserve_existing: true,
            }));
        }
        self.data_write.phase = 0;
        let sectors = u32::from(self.data_write.action_sectors.max(1));
        let consumed = if sectors == 1 && self.data_write.action_len != 0 {
            u32::from(self.data_write.action_len)
        } else {
            cmp::min(
                self.data_write.remaining,
                sectors.saturating_mul(SD_SECTOR_SIZE as u32),
            )
        };
        self.data_write.remaining -= consumed;
        self.data_write.payload_offset += consumed;
        self.data_write.sector_byte_offset = 0;
        if matches!(self.request, Some(FatRequest::UploadChunk { .. })) && self.upload.contiguous {
            let total_sectors = u32::from(self.data_write.sector_offset).saturating_add(sectors);
            let sectors_per_cluster = u32::from(volume.sectors_per_cluster);
            self.data_write.cluster = self
                .data_write
                .cluster
                .saturating_add(total_sectors / sectors_per_cluster);
            self.data_write.sector_offset = (total_sectors % sectors_per_cluster) as u8;
            return Ok(FatStep::Continue);
        }
        self.data_write.sector_offset = self.data_write.sector_offset.saturating_add(sectors as u8);
        if self.data_write.remaining == 0 {
            return Ok(FatStep::Continue);
        }
        if self.data_write.sector_offset < volume.sectors_per_cluster {
            return Ok(FatStep::Continue);
        }
        self.fat_read.start(self.data_write.cluster);
        self.fat_read_return = FatReadReturn::DataWrite;
        self.stage = CommandStage::ReadFat;
        Ok(FatStep::Continue)
    }

    pub(super) fn advance_zero_write(&mut self) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if self.zero_write.remaining == 0 {
            self.stage = CommandStage::Mutate;
            return Ok(FatStep::Continue);
        }
        let lba = cluster_to_lba(&volume, self.zero_write.cluster)?
            .saturating_add(self.zero_write.sector_offset as u32);
        match self.zero_write.phase {
            0 => {
                let room = SD_SECTOR_SIZE as u16 - self.zero_write.byte_offset;
                self.zero_write.action_len =
                    cmp::min(self.zero_write.remaining, u32::from(room)) as u16;
                if self.zero_write.byte_offset != 0
                    || self.zero_write.action_len as usize != SD_SECTOR_SIZE
                {
                    self.zero_write.phase = 1;
                    Ok(self.issue(FatIoAction::ReadSector {
                        lba,
                        buffer: FatBufferId::Sector,
                    }))
                } else {
                    self.workspace.sector.fill(0);
                    self.zero_write.phase = 2;
                    Ok(self.issue(FatIoAction::WriteSector {
                        lba,
                        buffer: FatBufferId::Sector,
                    }))
                }
            }
            1 => {
                let start = self.zero_write.byte_offset as usize;
                let end = start + self.zero_write.action_len as usize;
                self.workspace.sector[start..end].fill(0);
                self.zero_write.phase = 2;
                Ok(self.issue(FatIoAction::WriteSector {
                    lba,
                    buffer: FatBufferId::Sector,
                }))
            }
            _ => {
                self.zero_write.remaining -= u32::from(self.zero_write.action_len);
                self.zero_write.byte_offset = 0;
                self.zero_write.sector_offset = self.zero_write.sector_offset.saturating_add(1);
                self.zero_write.phase = 0;
                if self.zero_write.remaining == 0
                    || self.zero_write.sector_offset < volume.sectors_per_cluster
                {
                    return Ok(FatStep::Continue);
                }
                self.fat_read.start(self.zero_write.cluster);
                self.fat_read_return = FatReadReturn::ZeroWrite;
                self.stage = CommandStage::ReadFat;
                Ok(FatStep::Continue)
            }
        }
    }

    pub(super) fn after_zero_fat_read(&mut self, value: u32) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if !(2..super::super::FAT32_EOC).contains(&value)
            || value > volume.total_clusters.saturating_add(1)
        {
            return Err(SdFatError::ClusterChainTooLong);
        }
        self.zero_write.cluster = value;
        self.zero_write.sector_offset = 0;
        self.stage = CommandStage::ZeroWrite;
        Ok(FatStep::Continue)
    }

    pub(super) fn after_mutation_fat_read(&mut self, value: u32) -> Result<FatStep, SdFatError> {
        match self.fat_read_return {
            FatReadReturn::DataWrite => {
                let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
                if value >= super::super::FAT32_EOC {
                    return Err(SdFatError::ClusterChainTooLong);
                }
                if value < 2 || value > volume.total_clusters.saturating_add(1) {
                    return Err(SdFatError::BadCluster(value));
                }
                self.data_write.cluster = value;
                self.data_write.sector_offset = 0;
                self.stage = CommandStage::DataWrite;
                Ok(FatStep::Continue)
            }
            FatReadReturn::UploadCursor => {
                let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
                if value >= super::super::FAT32_EOC {
                    return Err(SdFatError::ClusterChainTooLong);
                }
                if value < 2 || value > volume.total_clusters.saturating_add(1) {
                    return Err(SdFatError::BadCluster(value));
                }
                self.upload.write_cluster = value;
                self.stage = CommandStage::Mutate;
                Ok(FatStep::Continue)
            }
            _ => Err(SdFatError::BadCluster(value)),
        }
    }

    pub(super) fn after_append_fat_read(&mut self, value: u32) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if !(2..super::super::FAT32_EOC).contains(&value)
            || value > volume.total_clusters.saturating_add(1)
        {
            return Err(SdFatError::ClusterChainTooLong);
        }
        self.mutation.tail_cluster = value;
        self.mutation.traverse_remaining = self.mutation.traverse_remaining.saturating_sub(1);
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }
}
