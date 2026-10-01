use super::MutationStage;
use crate::fat::engine::{
    CommandStage, FatEngine, FatReadReturn, FatRequest, FatResult, FatStep, FatWriteReturn,
};
use crate::fat::{clusters_for_size, SdFatError, FAT32_EOC, FAT32_EOC_WRITE, SD_SECTOR_SIZE};

impl FatEngine {
    pub(super) fn begin_truncate(&mut self) -> Result<FatStep, SdFatError> {
        let found = self.target.ok_or(SdFatError::NotFound)?;
        if found.record.is_dir() {
            return Err(SdFatError::IsDirectory);
        }
        let target_size = match self.request.as_ref() {
            Some(FatRequest::Truncate { size, .. }) => *size,
            _ => return Err(SdFatError::InvalidPath),
        };
        if target_size == found.record.size {
            return Ok(self.finish(FatResult::Done));
        }
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
        self.mutation.old_size = found.record.size;
        self.mutation.target_size = target_size;
        self.mutation.old_first = found.record.first_cluster;
        self.mutation.new_first = found.record.first_cluster;
        self.mutation.old_clusters =
            clusters_for_size(found.record.size as usize, cluster_size) as u32;
        self.mutation.target_clusters =
            clusters_for_size(target_size as usize, cluster_size) as u32;
        self.mutation.data_len = target_size;

        if self.mutation.target_clusters == 0 {
            if found.record.first_cluster >= 2 {
                self.free.start(
                    found.record.first_cluster,
                    self.mutation.old_clusters.saturating_add(32),
                );
                self.mutation.new_first = 0;
                self.mutation.stage = MutationStage::TruncateWaitFree;
                self.stage = CommandStage::Free;
                return Ok(FatStep::Continue);
            }
            self.mutation.new_first = 0;
            return self.finish_truncate();
        }
        if self.mutation.old_clusters == 0 {
            self.allocation.start(self.mutation.target_clusters, volume);
            self.mutation.stage = MutationStage::TruncateWaitAllocate;
            self.stage = CommandStage::Allocate;
            return Ok(FatStep::Continue);
        }
        if found.record.first_cluster < 2 {
            return Err(SdFatError::BadCluster(found.record.first_cluster));
        }
        self.mutation.tail_cluster = found.record.first_cluster;
        self.mutation.traverse_remaining =
            if self.mutation.target_clusters < self.mutation.old_clusters {
                self.mutation.target_clusters.saturating_sub(1)
            } else {
                self.mutation.old_clusters.saturating_sub(1)
            };
        self.mutation.stage = MutationStage::TruncateTraverse;
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }

    pub(super) fn advance_truncate_traverse(&mut self) -> Result<FatStep, SdFatError> {
        if self.mutation.traverse_remaining == 0 {
            if self.mutation.target_clusters > self.mutation.old_clusters {
                let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
                self.allocation.start(
                    self.mutation.target_clusters - self.mutation.old_clusters,
                    volume,
                );
                self.mutation.stage = MutationStage::TruncateWaitAllocate;
                self.stage = CommandStage::Allocate;
            } else if self.mutation.target_clusters < self.mutation.old_clusters {
                self.mutation.stage = MutationStage::TruncateReadFreeStart;
                self.stage = CommandStage::Mutate;
            } else {
                return self.start_truncate_zero();
            }
            return Ok(FatStep::Continue);
        }
        self.fat_read.start(self.mutation.tail_cluster);
        self.fat_read_return = FatReadReturn::TruncateTraverse;
        self.stage = CommandStage::ReadFat;
        Ok(FatStep::Continue)
    }

    pub(in crate::fat::engine) fn after_truncate_fat_read(
        &mut self,
        value: u32,
    ) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if self.fat_read_return == FatReadReturn::TruncateFreeStart {
            self.mutation.free_start = if value >= FAT32_EOC {
                0
            } else if value < 2 || value > volume.total_clusters.saturating_add(1) {
                return Err(SdFatError::BadCluster(value));
            } else {
                value
            };
            self.fat_write
                .start(self.mutation.tail_cluster, FAT32_EOC_WRITE);
            self.fat_write_return = FatWriteReturn::TruncateCut;
            self.mutation.stage = MutationStage::TruncateWaitCut;
            self.stage = CommandStage::WriteFat;
            return Ok(FatStep::Continue);
        }
        if !(2..FAT32_EOC).contains(&value) || value > volume.total_clusters.saturating_add(1) {
            return Err(SdFatError::ClusterChainTooLong);
        }
        self.mutation.tail_cluster = value;
        self.mutation.traverse_remaining = self.mutation.traverse_remaining.saturating_sub(1);
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }

    pub(super) fn finish_truncate_allocation(&mut self) -> Result<FatStep, SdFatError> {
        if self.mutation.old_clusters == 0 {
            self.mutation.new_first = self.allocation.first;
            self.mutation.tail_cluster = self.allocation.first;
            return self.start_truncate_zero();
        }
        self.fat_write
            .start(self.mutation.tail_cluster, self.allocation.first);
        self.fat_write_return = FatWriteReturn::TruncateLink;
        self.mutation.stage = MutationStage::TruncateWaitLink;
        self.stage = CommandStage::WriteFat;
        Ok(FatStep::Continue)
    }

    pub(super) fn read_truncate_free_start(&mut self) -> Result<FatStep, SdFatError> {
        self.fat_read.start(self.mutation.tail_cluster);
        self.fat_read_return = FatReadReturn::TruncateFreeStart;
        self.stage = CommandStage::ReadFat;
        Ok(FatStep::Continue)
    }

    pub(super) fn free_truncated_suffix(&mut self) -> Result<FatStep, SdFatError> {
        if self.mutation.free_start >= 2 {
            self.free.start(
                self.mutation.free_start,
                self.mutation
                    .old_clusters
                    .saturating_sub(self.mutation.target_clusters)
                    .saturating_add(32),
            );
            self.mutation.stage = MutationStage::TruncateWaitFree;
            self.stage = CommandStage::Free;
        } else {
            return self.start_truncate_zero();
        }
        Ok(FatStep::Continue)
    }

    pub(super) fn start_truncate_zero(&mut self) -> Result<FatStep, SdFatError> {
        if self.mutation.target_size > self.mutation.old_size {
            let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
            let cluster_size = SD_SECTOR_SIZE as u32 * u32::from(volume.sectors_per_cluster);
            let offset = self.mutation.old_size % cluster_size;
            let cluster = if self.mutation.old_clusters == 0 {
                self.mutation.new_first
            } else if offset == 0 {
                self.allocation.first
            } else {
                self.mutation.tail_cluster
            };
            self.zero_write.start(
                cluster,
                (offset / SD_SECTOR_SIZE as u32) as u8,
                (offset % SD_SECTOR_SIZE as u32) as u16,
                self.mutation.target_size - self.mutation.old_size,
            );
            self.mutation.stage = MutationStage::TruncateZero;
            self.stage = CommandStage::ZeroWrite;
            return Ok(FatStep::Continue);
        }
        if self.mutation.target_size > 0 {
            let byte_offset = (self.mutation.target_size % SD_SECTOR_SIZE as u32) as u16;
            if byte_offset != 0 {
                self.zero_write.start(
                    self.mutation.tail_cluster,
                    ((self.mutation.target_size / SD_SECTOR_SIZE as u32)
                        % u32::from(
                            self.volume
                                .ok_or(SdFatError::InvalidBootSector)?
                                .sectors_per_cluster,
                        )) as u8,
                    byte_offset,
                    SD_SECTOR_SIZE as u32 - u32::from(byte_offset),
                );
                self.mutation.stage = MutationStage::TruncateZero;
                self.stage = CommandStage::ZeroWrite;
                return Ok(FatStep::Continue);
            }
        }
        self.finish_truncate()
    }

    pub(super) fn finish_truncate(&mut self) -> Result<FatStep, SdFatError> {
        self.mutation.data_len = self.mutation.target_size;
        self.mutation.stage = MutationStage::WaitData;
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }
}
