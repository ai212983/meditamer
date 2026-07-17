use super::super::{
    cluster_to_lba, process_directory_slot, DirFound, DirectoryScanState, DirectorySlotOutcome,
    PathSegment, SdFatError, DIR_ENTRIES_PER_SECTOR, SD_SECTOR_SIZE,
};
use super::{
    CommandStage, FatBufferId, FatEngine, FatIoAction, FatReadReturn, FatRequest, FatResult,
    FatStageLabel, FatStep, ScanReturn,
};

pub(super) struct ResolveState {
    cluster: u32,
    segment: u8,
    count: u8,
}

impl ResolveState {
    pub(super) const fn new() -> Self {
        Self {
            cluster: 0,
            segment: 0,
            count: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.cluster = 0;
        self.segment = 0;
        self.count = 0;
    }

    pub(super) fn start(&mut self, root_cluster: u32, count: u8) {
        self.cluster = root_cluster;
        self.segment = 0;
        self.count = count;
    }

    pub(super) fn label(&self) -> FatStageLabel {
        FatStageLabel::ResolvePath
    }

    pub(super) fn cluster(&self) -> u32 {
        self.cluster
    }
}

pub(super) struct ScanState {
    cluster: u32,
    sector_offset: u8,
    visited: u32,
    target: PathSegment,
    short_target: [u8; 11],
    mode: ScanMode,
    directory: DirectoryScanState,
    result: Option<DirFound>,
    needed_free_slots: usize,
    awaiting_sector: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanMode {
    Name,
    Free,
    Short,
    Empty,
}

impl ScanState {
    pub(super) fn new() -> Self {
        Self {
            cluster: 0,
            sector_offset: 0,
            visited: 0,
            target: PathSegment::EMPTY,
            short_target: [0; 11],
            mode: ScanMode::Name,
            directory: DirectoryScanState::new(),
            result: None,
            needed_free_slots: 0,
            awaiting_sector: false,
        }
    }

    pub(super) fn reset(&mut self) {
        self.cluster = 0;
        self.sector_offset = 0;
        self.visited = 0;
        self.target = PathSegment::EMPTY;
        self.short_target = [0; 11];
        self.mode = ScanMode::Name;
        self.directory.free_slots = None;
        self.directory
            .free_run
            .fill(super::super::DirLocation::ZERO);
        self.directory.free_run_len = 0;
        self.directory.reached_directory_end = false;
        self.directory.lfn.clear();
        self.result = None;
        self.needed_free_slots = 0;
        self.awaiting_sector = false;
    }

    pub(super) fn start_name(
        &mut self,
        cluster: u32,
        target: PathSegment,
        needed_free_slots: usize,
    ) {
        self.reset();
        self.cluster = cluster;
        self.target = target;
        self.needed_free_slots = needed_free_slots;
    }

    pub(super) fn start_free(&mut self, cluster: u32, needed_free_slots: usize) {
        self.reset();
        self.cluster = cluster;
        self.mode = ScanMode::Free;
        self.needed_free_slots = needed_free_slots;
    }

    pub(super) fn start_short(&mut self, cluster: u32, short_target: [u8; 11]) {
        self.reset();
        self.cluster = cluster;
        self.mode = ScanMode::Short;
        self.short_target = short_target;
    }

    pub(super) fn start_empty(&mut self, cluster: u32) {
        self.reset();
        self.cluster = cluster;
        self.mode = ScanMode::Empty;
    }

    pub(super) fn first_free(&self) -> Option<super::super::DirLocation> {
        self.directory.free_slots.map(|slots| slots[0])
    }

    pub(super) fn free_slots(
        &self,
    ) -> Option<[super::super::DirLocation; super::super::MAX_LFN_SLOTS + 1]> {
        self.directory.free_slots
    }

    pub(super) fn found(&self) -> Option<DirFound> {
        self.result
    }

    pub(super) fn tail_cluster(&self) -> u32 {
        self.cluster
    }

    pub(super) fn label(&self) -> FatStageLabel {
        FatStageLabel::ScanDirectory
    }
}

pub(super) struct FatReadState {
    cluster: u32,
    awaiting_sector: bool,
}

impl FatReadState {
    pub(super) const fn new() -> Self {
        Self {
            cluster: 0,
            awaiting_sector: false,
        }
    }

    pub(super) fn reset(&mut self) {
        self.cluster = 0;
        self.awaiting_sector = false;
    }

    pub(super) fn start(&mut self, cluster: u32) {
        self.cluster = cluster;
        self.awaiting_sector = false;
    }
}

impl FatEngine {
    pub(super) fn advance_resolve(&mut self) -> Result<FatStep, SdFatError> {
        if self.resolve.segment >= self.resolve.count {
            return self.after_resolve();
        }
        let target = self.workspace.segments[self.resolve.segment as usize];
        self.scan.start_name(self.resolve.cluster, target, 0);
        self.scan_return = ScanReturn::Resolve;
        self.stage = CommandStage::FindTarget;
        Ok(FatStep::Continue)
    }

    pub(super) fn advance_find_target(&mut self) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        if !self.scan.awaiting_sector {
            if self.scan.visited > volume.total_clusters.saturating_add(2) {
                return Err(SdFatError::ClusterChainTooLong);
            }
            let lba = cluster_to_lba(&volume, self.scan.cluster)?
                .saturating_add(self.scan.sector_offset as u32);
            self.scan.awaiting_sector = true;
            return Ok(self.issue(FatIoAction::ReadSector {
                lba,
                buffer: FatBufferId::Sector,
            }));
        }

        self.scan.awaiting_sector = false;
        let lba = cluster_to_lba(&volume, self.scan.cluster)?
            .saturating_add(self.scan.sector_offset as u32);
        for slot in 0..DIR_ENTRIES_PER_SECTOR {
            if self.scan.mode == ScanMode::Empty
                && directory_slot_is_non_dot_entry(&self.workspace.sector, slot)
            {
                self.scan.result = Some(raw_short_match(&self.workspace.sector, lba, slot as u8));
                return self.after_scan();
            }
            if self.scan.mode == ScanMode::Short
                && raw_short_name_matches(&self.workspace.sector, slot, &self.scan.short_target)
            {
                self.scan.result = Some(raw_short_match(&self.workspace.sector, lba, slot as u8));
                return self.after_scan();
            }
            let target = if self.scan.mode == ScanMode::Name {
                Some(&self.scan.target)
            } else {
                None
            };
            match process_directory_slot(
                &mut self.scan.directory,
                &self.workspace.sector,
                lba,
                slot as u8,
                target,
                self.scan.needed_free_slots,
            ) {
                DirectorySlotOutcome::Found(found) => {
                    self.scan.result = Some(found);
                    return self.after_scan();
                }
                DirectorySlotOutcome::EarlyReturnFree => return self.after_scan(),
                DirectorySlotOutcome::Continue => {}
            }
        }

        self.scan.sector_offset = self.scan.sector_offset.saturating_add(1);
        if self.scan.sector_offset < volume.sectors_per_cluster {
            return Ok(FatStep::Continue);
        }
        self.scan.sector_offset = 0;
        self.scan.visited = self.scan.visited.saturating_add(1);
        self.fat_read.start(self.scan.cluster);
        self.fat_read_return = FatReadReturn::Scan;
        self.stage = CommandStage::ReadFat;
        Ok(FatStep::Continue)
    }

    pub(super) fn advance_fat_read(&mut self) -> Result<FatStep, SdFatError> {
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        let byte_offset = self.fat_read.cluster as u64 * 4;
        let sector_offset = (byte_offset / SD_SECTOR_SIZE as u64) as u32;
        let index = (byte_offset % SD_SECTOR_SIZE as u64) as usize;
        if sector_offset >= volume.fat_size_sectors || index + 4 > SD_SECTOR_SIZE {
            return Err(SdFatError::BadCluster(self.fat_read.cluster));
        }
        if !self.fat_read.awaiting_sector {
            self.fat_read.awaiting_sector = true;
            return Ok(self.issue(FatIoAction::ReadSector {
                lba: volume.fat_start_lba.saturating_add(sector_offset),
                buffer: FatBufferId::Sector,
            }));
        }
        self.fat_read.awaiting_sector = false;
        let raw = u32::from_le_bytes([
            self.workspace.sector[index],
            self.workspace.sector[index + 1],
            self.workspace.sector[index + 2],
            self.workspace.sector[index + 3],
        ]) & 0x0FFF_FFFF;
        self.after_fat_read(raw)
    }

    fn after_fat_read(&mut self, value: u32) -> Result<FatStep, SdFatError> {
        if matches!(
            self.fat_read_return,
            FatReadReturn::DataWrite | FatReadReturn::UploadCursor
        ) {
            return self.after_mutation_fat_read(value);
        }
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        let next = if value >= super::super::FAT32_EOC {
            None
        } else if value < 2 || value > volume.total_clusters.saturating_add(1) {
            return Err(SdFatError::BadCluster(value));
        } else {
            Some(value)
        };
        match self.fat_read_return {
            FatReadReturn::Scan => {
                if let Some(next) = next {
                    self.scan.cluster = next;
                    self.stage = CommandStage::FindTarget;
                    Ok(FatStep::Continue)
                } else {
                    self.scan.result = None;
                    self.after_scan()
                }
            }
            FatReadReturn::List => {
                if let Some(next) = next {
                    self.list.cluster = next;
                    self.list.sector_offset = 0;
                    self.list.visited = self.list.visited.saturating_add(1);
                    self.stage = CommandStage::List;
                    Ok(FatStep::Continue)
                } else {
                    Ok(self.finish(FatResult::Listed {
                        count: self.list.count as u8,
                    }))
                }
            }
            FatReadReturn::Read => {
                let next = next.ok_or(SdFatError::ClusterChainTooLong)?;
                self.read.cluster = next;
                self.read.sector_offset = 0;
                self.read.visited = self.read.visited.saturating_add(1);
                self.stage = CommandStage::Read;
                Ok(FatStep::Continue)
            }
            FatReadReturn::DataWrite | FatReadReturn::UploadCursor => {
                Err(SdFatError::BadCluster(value))
            }
            FatReadReturn::AppendTraverse => self.after_append_fat_read(value),
            FatReadReturn::TruncateTraverse | FatReadReturn::TruncateFreeStart => {
                self.after_truncate_fat_read(value)
            }
            FatReadReturn::ZeroWrite => self.after_zero_fat_read(value),
        }
    }

    fn after_scan(&mut self) -> Result<FatStep, SdFatError> {
        match self.scan_return {
            ScanReturn::Resolve => {
                let found = self.scan.result.ok_or(SdFatError::NotFound)?;
                if !found.record.is_dir() {
                    return Err(SdFatError::NotDirectory);
                }
                let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
                self.resolve.cluster = if found.record.first_cluster >= 2 {
                    found.record.first_cluster
                } else {
                    volume.root_cluster
                };
                self.resolve.segment = self.resolve.segment.saturating_add(1);
                self.stage = CommandStage::Resolve;
                Ok(FatStep::Continue)
            }
            ScanReturn::Target => {
                self.target = self.scan.result;
                self.after_target_scan()
            }
            ScanReturn::Mutation => self.after_mutation_scan(),
        }
    }

    fn after_resolve(&mut self) -> Result<FatStep, SdFatError> {
        if self.mutation.is_rename_resolve() {
            let count = self.workspace.segment_count as usize;
            let target = self.workspace.segments[count - 1];
            self.mutation.parent_cluster = self.resolve.cluster;
            self.scan.start_name(self.resolve.cluster, target, 0);
            self.scan_return = ScanReturn::Mutation;
            self.mutation.stage = super::mutate::MutationStage::RenameTarget;
            self.stage = CommandStage::FindTarget;
            return Ok(FatStep::Continue);
        }
        match self.request.as_ref().ok_or(SdFatError::InvalidPath)? {
            FatRequest::List { .. } => {
                self.list.start(self.resolve.cluster);
                self.stage = CommandStage::List;
            }
            _ => {
                let count = self.workspace.segment_count as usize;
                let target = self.workspace.segments[count - 1];
                let needed_free =
                    usize::from(matches!(self.request, Some(FatRequest::Write { .. })));
                self.scan
                    .start_name(self.resolve.cluster, target, needed_free);
                self.scan_return = ScanReturn::Target;
                self.stage = CommandStage::FindTarget;
            }
        }
        Ok(FatStep::Continue)
    }

    fn after_target_scan(&mut self) -> Result<FatStep, SdFatError> {
        match self.request.as_ref().ok_or(SdFatError::InvalidPath)? {
            FatRequest::Write { .. }
            | FatRequest::Mkdir { .. }
            | FatRequest::Remove { .. }
            | FatRequest::Append { .. }
            | FatRequest::Truncate { .. }
            | FatRequest::UploadBegin { .. } => self.begin_mutation(self.target),
            FatRequest::Rename { .. } => self.begin_rename(),
            FatRequest::Stat { .. } => {
                let found = self.target.ok_or(SdFatError::NotFound)?;
                Ok(self.finish(FatResult::Stat(to_public_entry(found))))
            }
            FatRequest::Read {
                output,
                output_capacity,
                ..
            } => {
                let found = self.target.ok_or(SdFatError::NotFound)?;
                if found.record.is_dir() {
                    return Err(SdFatError::IsDirectory);
                }
                if found.record.size > *output_capacity {
                    return Err(SdFatError::BufferTooSmall {
                        needed: found.record.size as usize,
                    });
                }
                if found.record.size == 0 {
                    return Ok(self.finish(FatResult::Read { bytes: 0 }));
                }
                if found.record.first_cluster < 2 {
                    return Err(SdFatError::BadCluster(found.record.first_cluster));
                }
                self.read
                    .start(found.record.first_cluster, found.record.size, *output);
                self.stage = CommandStage::Read;
                Ok(FatStep::Continue)
            }
            _ => Ok(self.finish(FatResult::Error(super::FatEngineError::UnsupportedRequest))),
        }
    }
}

fn raw_short_name_matches(sector: &[u8; SD_SECTOR_SIZE], slot: usize, target: &[u8; 11]) -> bool {
    let base = slot * super::super::DIR_ENTRY_SIZE;
    let first = sector[base];
    if first == 0x00 || first == 0xE5 {
        return false;
    }
    let attr = sector[base + 11];
    if attr == super::super::ATTR_LONG_NAME || (attr & super::super::ATTR_VOLUME) != 0 {
        return false;
    }
    &sector[base..base + 11] == target
}

fn raw_short_match(sector: &[u8; SD_SECTOR_SIZE], lba: u32, slot: u8) -> DirFound {
    let base = slot as usize * super::super::DIR_ENTRY_SIZE;
    let empty_lfn = super::super::LfnState::new();
    let record = super::super::parse_record(sector, base, &empty_lfn);
    DirFound {
        short_location: super::super::DirLocation { lba, slot },
        lfn_locations: [super::super::DirLocation::ZERO; super::super::MAX_LFN_SLOTS],
        lfn_count: 0,
        record,
    }
}

fn directory_slot_is_non_dot_entry(sector: &[u8; SD_SECTOR_SIZE], slot: usize) -> bool {
    let base = slot * super::super::DIR_ENTRY_SIZE;
    let first = sector[base];
    if first == 0x00 || first == 0xE5 {
        return false;
    }
    let attr = sector[base + 11];
    if attr == super::super::ATTR_LONG_NAME || (attr & super::super::ATTR_VOLUME) != 0 {
        return false;
    }
    let raw = &sector[base..base + 11];
    raw != b".          " && raw != b"..         "
}

fn to_public_entry(found: DirFound) -> super::super::FatDirEntry {
    super::super::FatDirEntry {
        name: found.record.display_name,
        name_len: found.record.display_name_len,
        is_dir: found.record.is_dir(),
        size: found.record.size,
    }
}
