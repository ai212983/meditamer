use super::directory_encode::{write_dot_entries, write_lfn_to_sector, write_record_to_sector};
use super::MutationStage;
use crate::fat::engine::{
    CommandStage, FatBufferId, FatEngine, FatIoAction, FatRequest, FatResult, FatStep,
    FatWriteReturn,
};
use crate::fat::{
    cluster_to_lba, path_segment_to_name, DirFound, DirLocation, DirRecord, SdFatError,
    ATTR_DIRECTORY, DIR_ENTRIES_PER_SECTOR, MAX_LFN_SLOTS,
};

impl FatEngine {
    pub(super) fn link_extended_directory(&mut self) -> Result<FatStep, SdFatError> {
        self.mutation.directory_new_cluster = self.allocation.first;
        if self.mutation.directory_new_cluster < 2 {
            return Err(SdFatError::NoFreeCluster);
        }
        self.fat_write.start(
            self.mutation.directory_tail,
            self.mutation.directory_new_cluster,
        );
        self.fat_write_return = FatWriteReturn::DirectoryLink;
        self.mutation.stage = MutationStage::WaitDirectoryLink;
        self.stage = CommandStage::WriteFat;
        Ok(FatStep::Continue)
    }

    pub(super) fn zero_extended_directory(&mut self) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if self.mutation.stage == MutationStage::WaitDirectoryLink {
            self.mutation.stage = MutationStage::ZeroDirectoryCluster;
            self.mutation.zero_sector = 0;
            self.mutation.directory_sector_pending = false;
        }
        if self.mutation.directory_sector_pending {
            self.mutation.directory_sector_pending = false;
            self.mutation.zero_sector = self.mutation.zero_sector.saturating_add(1);
        }
        if self.mutation.zero_sector < volume.sectors_per_cluster {
            self.workspace.sector.fill(0);
            let lba = cluster_to_lba(&volume, self.mutation.directory_new_cluster)?
                .saturating_add(self.mutation.zero_sector as u32);
            self.mutation.directory_sector_pending = true;
            return Ok(self.issue(FatIoAction::WriteSector {
                lba,
                buffer: FatBufferId::Sector,
            }));
        }

        let first_lba = cluster_to_lba(&volume, self.mutation.directory_new_cluster)?;
        let mut slots = [DirLocation::ZERO; MAX_LFN_SLOTS + 1];
        for (index, location) in slots
            .iter_mut()
            .take(self.mutation.needed_slots as usize)
            .enumerate()
        {
            *location = DirLocation {
                lba: first_lba + (index / DIR_ENTRIES_PER_SECTOR) as u32,
                slot: (index % DIR_ENTRIES_PER_SECTOR) as u8,
            };
        }
        self.prepare_new_entry(slots)
    }

    pub(super) fn prepare_new_entry(
        &mut self,
        slots: [DirLocation; MAX_LFN_SLOTS + 1],
    ) -> Result<FatStep, SdFatError> {
        let lfn_count = self.mutation.needed_slots.saturating_sub(1);
        let mut lfn_locations = [DirLocation::ZERO; MAX_LFN_SLOTS];
        lfn_locations[..lfn_count as usize].copy_from_slice(&slots[..lfn_count as usize]);
        let target = self.workspace.segments[self.workspace.segment_count as usize - 1];
        let is_directory = matches!(self.request, Some(FatRequest::Mkdir { .. }));
        let rename_source = if self.is_rename_operation() {
            self.mutation.rename_source
        } else {
            None
        };
        self.target = Some(DirFound {
            short_location: slots[lfn_count as usize],
            lfn_locations,
            lfn_count,
            record: DirRecord {
                short_name: self.mutation.short_name,
                display_name: path_segment_to_name(target),
                display_name_len: target.len,
                attr: rename_source.map_or_else(
                    || {
                        if is_directory {
                            ATTR_DIRECTORY
                        } else {
                            0x20
                        }
                    },
                    |source| source.record.attr,
                ),
                first_cluster: rename_source.map_or(0, |source| source.record.first_cluster),
                size: rename_source.map_or(0, |source| source.record.size),
            },
        });
        if is_directory {
            let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
            self.allocation.start(1, volume);
            self.mutation.stage = MutationStage::WaitMkdirAllocate;
            self.stage = CommandStage::Allocate;
        } else if let Some(source) = rename_source {
            self.mutation.new_first = source.record.first_cluster;
            self.mutation.data_len = source.record.size;
            self.mutation.stage = MutationStage::WaitData;
            self.stage = CommandStage::Mutate;
        } else {
            self.mutation.stage = MutationStage::Start;
            self.stage = CommandStage::Mutate;
        }
        Ok(FatStep::Continue)
    }

    pub(super) fn initialize_mkdir(&mut self) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if self.mutation.stage == MutationStage::WaitMkdirAllocate {
            self.mutation.new_first = self.allocation.first;
            self.mutation.zero_sector = 0;
            self.mutation.directory_sector_pending = false;
            self.mutation.stage = MutationStage::InitializeMkdir;
        }
        if self.mutation.directory_sector_pending {
            self.mutation.directory_sector_pending = false;
            self.mutation.zero_sector = self.mutation.zero_sector.saturating_add(1);
        }
        if self.mutation.zero_sector < volume.sectors_per_cluster {
            self.workspace.sector.fill(0);
            if self.mutation.zero_sector == 0 {
                write_dot_entries(
                    &mut self.workspace.sector,
                    self.mutation.new_first,
                    self.mutation.parent_cluster,
                    volume.root_cluster,
                );
            }
            let lba = cluster_to_lba(&volume, self.mutation.new_first)?
                .saturating_add(self.mutation.zero_sector as u32);
            self.mutation.directory_sector_pending = true;
            return Ok(self.issue(FatIoAction::WriteSector {
                lba,
                buffer: FatBufferId::Sector,
            }));
        }
        self.mutation.stage = MutationStage::WaitData;
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }

    pub(super) fn mutation_read_directory(&mut self) -> Result<FatStep, SdFatError> {
        let found = self.target.ok_or(SdFatError::NotFound)?;
        let location = if self.mutation.new_entry && self.mutation.entry_index < found.lfn_count {
            found.lfn_locations[self.mutation.entry_index as usize]
        } else {
            found.short_location
        };
        if !self.mutation.directory_sector_pending {
            self.mutation.directory_sector_pending = true;
            return Ok(self.issue(FatIoAction::ReadSector {
                lba: location.lba,
                buffer: FatBufferId::Sector,
            }));
        }
        self.mutation.directory_sector_pending = false;
        let mut record = found.record;
        record.first_cluster = self.mutation.new_first;
        record.size = self.mutation.data_len;
        if self.mutation.new_entry && self.mutation.entry_index < found.lfn_count {
            write_lfn_to_sector(
                &mut self.workspace.sector,
                location,
                record,
                self.mutation.entry_index,
                found.lfn_count,
                self.mutation.lfn_len as usize,
            )?;
        } else {
            write_record_to_sector(&mut self.workspace.sector, location, record);
        }
        self.mutation.stage = MutationStage::WriteDirectory;
        Ok(self.issue(FatIoAction::WriteSector {
            lba: location.lba,
            buffer: FatBufferId::Sector,
        }))
    }

    pub(super) fn mutation_write_directory_done(&mut self) -> Result<FatStep, SdFatError> {
        let found = self.target.ok_or(SdFatError::NotFound)?;
        if self.mutation.new_entry && self.mutation.entry_index < found.lfn_count {
            self.mutation.entry_index = self.mutation.entry_index.saturating_add(1);
            self.mutation.stage = MutationStage::ReadDirectory;
            return Ok(FatStep::Continue);
        }
        if self.is_rename_operation() && self.mutation.new_entry {
            self.target = self.mutation.rename_source;
            self.mutation.new_entry = false;
            self.mutation.delete_return = 2;
            self.mutation.delete_index = 0;
            self.mutation.directory_sector_pending = false;
            self.mutation.stage = MutationStage::RenameDeleteSource;
            self.stage = CommandStage::Mutate;
            return Ok(FatStep::Continue);
        }
        if matches!(self.request, Some(FatRequest::UploadBegin { .. })) {
            let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
            self.upload.valid = true;
            self.upload.volume = volume;
            self.upload.location = found.short_location;
            self.upload.parent_cluster = self.mutation.parent_cluster;
            self.upload.record = found.record;
            self.upload.record.first_cluster = self.mutation.new_first;
            self.upload.record.size = self.mutation.data_len;
            self.upload.allocated_clusters = self.mutation.target_clusters;
            self.upload.tail_cluster = if self.mutation.target_clusters == 0 {
                0
            } else {
                self.allocation.previous
            };
            self.upload.write_cluster = self.mutation.new_first;
            self.upload.contiguous = self.allocation.contiguous;
        }
        if matches!(self.request, Some(FatRequest::UploadCommit { .. })) {
            return self.begin_cached_upload_rename();
        }
        Ok(self.finish(FatResult::Done))
    }
}
