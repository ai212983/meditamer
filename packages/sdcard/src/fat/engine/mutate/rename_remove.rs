impl FatEngine {
    pub(super) fn begin_rename(&mut self) -> Result<FatStep, SdFatError> {
        let source = self.target.ok_or(SdFatError::NotFound)?;
        let source_parent = self.resolve.cluster();
        let (dst_path, dst_len) = self.rename_destination()?;
        self.mutation.reset();
        self.mutation.rename_source = Some(source);
        self.mutation.rename_source_parent = source_parent;
        self.prepare_rename_destination(dst_path, dst_len)
    }

    pub(super) fn prepare_rename_destination(
        &mut self,
        dst_path: [u8; super::super::super::SD_PATH_MAX],
        dst_len: u8,
    ) -> Result<FatStep, SdFatError> {
        let len = usize::from(dst_len);
        if len == 0 || len > dst_path.len() {
            return Err(SdFatError::InvalidPath);
        }
        let path = core::str::from_utf8(&dst_path[..len]).map_err(|_| SdFatError::InvalidPath)?;
        let count = super::super::parse_path(path, &mut self.workspace.segments)?;
        if count == 0 {
            return Err(SdFatError::InvalidPath);
        }
        self.workspace.segment_count = count as u8;
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        self.resolve
            .start(volume.root_cluster, count.saturating_sub(1) as u8);
        self.mutation.stage = MutationStage::RenameResolve;
        self.stage = CommandStage::Resolve;
        Ok(FatStep::Continue)
    }

    fn begin_new_file_name(&mut self) -> Result<FatStep, SdFatError> {
        let target = self.workspace.segments[self.workspace.segment_count as usize - 1];
        self.mutation.new_entry = true;
        if let Ok(short_name) = encode_short_name(target.as_bytes()) {
            self.mutation.short_name = short_name;
            self.mutation.lfn_len = 0;
            self.mutation.needed_slots = 1;
            return self.reserve_new_entry_slots();
        }

        let lfn_len = utf16_len(target.as_bytes())?;
        self.mutation.lfn_len = lfn_len as u16;
        self.mutation.needed_slots = ((lfn_len + 12) / 13 + 1) as u8;
        self.mutation.alias_attempt = 1;
        self.begin_alias_scan()
    }

    fn begin_alias_scan(&mut self) -> Result<FatStep, SdFatError> {
        if self.mutation.alias_attempt >= 10_000 {
            return Err(SdFatError::DirFull);
        }
        let target = self.workspace.segments[self.workspace.segment_count as usize - 1];
        self.mutation.short_name = make_short_alias(target.as_bytes(), self.mutation.alias_attempt);
        self.scan
            .start_short(self.mutation.parent_cluster, self.mutation.short_name);
        self.scan_return = ScanReturn::Mutation;
        self.mutation.stage = MutationStage::WaitAliasScan;
        self.stage = CommandStage::FindTarget;
        Ok(FatStep::Continue)
    }

    fn reserve_new_entry_slots(&mut self) -> Result<FatStep, SdFatError> {
        if self.mutation.needed_slots == 1 {
            if let Some(location) = self.scan.first_free() {
                let mut slots = [DirLocation::ZERO; MAX_LFN_SLOTS + 1];
                slots[0] = location;
                return self.prepare_new_entry(slots);
            }
        }
        self.scan.start_free(
            self.mutation.parent_cluster,
            self.mutation.needed_slots as usize,
        );
        self.scan_return = ScanReturn::Mutation;
        self.mutation.stage = MutationStage::WaitFreeScan;
        self.stage = CommandStage::FindTarget;
        Ok(FatStep::Continue)
    }

    pub(super) fn after_mutation_scan(&mut self) -> Result<FatStep, SdFatError> {
        match self.mutation.stage {
            MutationStage::WaitAliasScan => {
                if self.scan.found().is_some() {
                    self.mutation.alias_attempt = self.mutation.alias_attempt.saturating_add(1);
                    self.begin_alias_scan()
                } else {
                    self.reserve_new_entry_slots()
                }
            }
            MutationStage::WaitFreeScan => {
                if let Some(slots) = self.scan.free_slots() {
                    self.prepare_new_entry(slots)
                } else {
                    let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
                    self.mutation.directory_tail = self.scan.tail_cluster();
                    self.allocation.start(1, volume);
                    self.mutation.stage = MutationStage::WaitDirectoryAllocate;
                    self.stage = CommandStage::Allocate;
                    Ok(FatStep::Continue)
                }
            }
            MutationStage::WaitEmptyScan => {
                if self.scan.found().is_some() {
                    Err(SdFatError::NotEmpty)
                } else {
                    self.free_remove_chain_or_delete()
                }
            }
            MutationStage::RenameTarget => self.after_rename_target_scan(),
            _ => Err(SdFatError::InvalidPath),
        }
    }

    fn after_rename_target_scan(&mut self) -> Result<FatStep, SdFatError> {
        let source = self.mutation.rename_source.ok_or(SdFatError::NotFound)?;
        if source.record.is_dir()
            && self.mutation.rename_source_parent != self.mutation.parent_cluster
        {
            return Err(SdFatError::CrossDirectoryRenameUnsupported);
        }
        let replace = self.rename_replace();
        if let Some(destination) = self.scan.found() {
            let same_entry = destination.short_location.lba == source.short_location.lba
                && destination.short_location.slot == source.short_location.slot;
            if same_entry {
                return if replace {
                    Ok(self.finish_rename_operation())
                } else {
                    Err(SdFatError::AlreadyExists)
                };
            }
            if !replace {
                return Err(SdFatError::AlreadyExists);
            }
            if destination.record.is_dir() {
                return Err(SdFatError::IsDirectory);
            }
            self.target = Some(destination);
            if destination.record.first_cluster >= 2 {
                let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
                let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
                let count = clusters_for_size(destination.record.size as usize, cluster_size);
                self.free.start(
                    destination.record.first_cluster,
                    count.saturating_add(32) as u32,
                );
                self.mutation.stage = MutationStage::RenameWaitDestFree;
                self.stage = CommandStage::Free;
                return Ok(FatStep::Continue);
            }
            return self.begin_rename_replacement();
        }
        self.begin_rename_destination_entry()
    }

    fn begin_rename_dest_delete(&mut self) -> Result<FatStep, SdFatError> {
        self.mutation.delete_return = 1;
        self.mutation.delete_index = 0;
        self.mutation.directory_sector_pending = false;
        self.mutation.stage = MutationStage::RenameDeleteDest;
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }

    fn begin_rename_replacement(&mut self) -> Result<FatStep, SdFatError> {
        if matches!(self.request, Some(FatRequest::UploadCommit { .. })) {
            self.begin_upload_commit_replace()
        } else {
            self.begin_rename_dest_delete()
        }
    }

    fn begin_rename_destination_entry(&mut self) -> Result<FatStep, SdFatError> {
        self.target = None;
        self.begin_new_file_name()
    }

    fn begin_remove(&mut self) -> Result<FatStep, SdFatError> {
        let found = self.target.ok_or(SdFatError::NotFound)?;
        if found.record.is_dir() {
            if found.record.first_cluster < 2 {
                return Err(SdFatError::BadCluster(found.record.first_cluster));
            }
            self.scan.start_empty(found.record.first_cluster);
            self.scan_return = ScanReturn::Mutation;
            self.mutation.stage = MutationStage::WaitEmptyScan;
            self.stage = CommandStage::FindTarget;
            Ok(FatStep::Continue)
        } else {
            self.free_remove_chain_or_delete()
        }
    }

    fn free_remove_chain_or_delete(&mut self) -> Result<FatStep, SdFatError> {
        let found = self.target.ok_or(SdFatError::NotFound)?;
        if found.record.first_cluster < 2 {
            return self.begin_delete_entry();
        }
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        let max_steps = if found.record.is_dir() {
            volume.total_clusters.saturating_add(2)
        } else {
            let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
            clusters_for_size(found.record.size as usize, cluster_size) as u32 + 32
        };
        self.free.start(found.record.first_cluster, max_steps);
        self.mutation.stage = MutationStage::WaitRemoveFree;
        self.stage = CommandStage::Free;
        Ok(FatStep::Continue)
    }

    fn begin_delete_entry(&mut self) -> Result<FatStep, SdFatError> {
        self.mutation.delete_index = 0;
        self.mutation.directory_sector_pending = false;
        self.mutation.stage = MutationStage::DeleteEntry;
        self.stage = CommandStage::Mutate;
        Ok(FatStep::Continue)
    }

    fn advance_delete_entry(&mut self) -> Result<FatStep, SdFatError> {
        let found = self.target.ok_or(SdFatError::NotFound)?;
        let total = found.lfn_count.saturating_add(1);
        if self.mutation.delete_index >= total {
            return match self.mutation.delete_return {
                1 => self.begin_rename_destination_entry(),
                2 => Ok(self.finish_rename_operation()),
                _ => Ok(self.finish(FatResult::Done)),
            };
        }
        let location = if self.mutation.delete_index < found.lfn_count {
            found.lfn_locations[self.mutation.delete_index as usize]
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
        let base = location.slot as usize * super::super::DIR_ENTRY_SIZE;
        self.workspace.sector[base] = 0xE5;
        self.mutation.delete_index = self.mutation.delete_index.saturating_add(1);
        Ok(self.issue(FatIoAction::WriteSector {
            lba: location.lba,
            buffer: FatBufferId::Sector,
        }))
    }
}
