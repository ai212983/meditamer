use super::super::{
    first_fat_partition_lba, parse_fat_size, parse_sectors_per_cluster, parse_total_sectors,
    Fat32Volume, SdFatError, SD_SECTOR_SIZE,
};
use super::{CommandStage, FatBufferId, FatEngine, FatIoAction, FatStageLabel, FatStep};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    ReadMbr,
    ParseMbr,
    ReadBoot,
    ParseBoot,
}

pub(super) struct MountStage {
    state: State,
    partition_lba: u32,
    tried_superfloppy: bool,
}

impl MountStage {
    pub(super) const fn new() -> Self {
        Self {
            state: State::ReadMbr,
            partition_lba: 0,
            tried_superfloppy: false,
        }
    }

    pub(super) fn reset(&mut self) {
        self.state = State::ReadMbr;
        self.partition_lba = 0;
        self.tried_superfloppy = false;
    }

    pub(super) fn label(&self) -> FatStageLabel {
        match self.state {
            State::ReadMbr | State::ParseMbr => FatStageLabel::MountMbr,
            State::ReadBoot | State::ParseBoot => FatStageLabel::MountBoot,
        }
    }
}

impl FatEngine {
    pub(super) fn advance_mount(&mut self) -> Result<FatStep, SdFatError> {
        match self.mount.state {
            State::ReadMbr => {
                self.mount.state = State::ParseMbr;
                Ok(self.issue(FatIoAction::ReadSector {
                    lba: 0,
                    buffer: FatBufferId::Sector,
                }))
            }
            State::ParseMbr => {
                self.mount.partition_lba =
                    first_fat_partition_lba(&self.workspace.sector).unwrap_or(0);
                self.mount.tried_superfloppy = self.mount.partition_lba == 0;
                self.mount.state = State::ReadBoot;
                Ok(FatStep::Continue)
            }
            State::ReadBoot => {
                self.mount.state = State::ParseBoot;
                Ok(self.issue(FatIoAction::ReadSector {
                    lba: self.mount.partition_lba,
                    buffer: FatBufferId::Sector,
                }))
            }
            State::ParseBoot => {
                match parse_boot(self.mount.partition_lba, &self.workspace.sector) {
                    Ok(volume) => {
                        self.volume = Some(volume);
                        self.prepare_path()?;
                        Ok(FatStep::Continue)
                    }
                    Err(_) if !self.mount.tried_superfloppy => {
                        self.mount.partition_lba = 0;
                        self.mount.tried_superfloppy = true;
                        self.mount.state = State::ReadBoot;
                        Ok(FatStep::Continue)
                    }
                    Err(err) => Err(err),
                }
            }
        }
    }

    fn prepare_path(&mut self) -> Result<(), SdFatError> {
        let request = self.request.as_ref().ok_or(SdFatError::InvalidPath)?;
        let (path, path_len) = request.path();
        let len = usize::from(path_len);
        if len > path.len() {
            return Err(SdFatError::InvalidPath);
        }
        let path = core::str::from_utf8(&path[..len]).map_err(|_| SdFatError::InvalidPath)?;
        let count = super::super::parse_path(path, &mut self.workspace.segments)?;
        self.workspace.segment_count = count as u8;

        let resolve_count = match request {
            super::FatRequest::List { .. } => count,
            _ if count == 0 => return Err(SdFatError::InvalidPath),
            _ => count - 1,
        };
        let volume = self.volume.ok_or(SdFatError::InvalidBootSector)?;
        self.resolve.start(volume.root_cluster, resolve_count as u8);
        self.stage = CommandStage::Resolve;
        Ok(())
    }
}

fn parse_boot(
    partition_start_lba: u32,
    boot: &[u8; SD_SECTOR_SIZE],
) -> Result<Fat32Volume, SdFatError> {
    if boot[510] != 0x55 || boot[511] != 0xAA {
        return Err(SdFatError::InvalidBootSector);
    }
    let bytes_per_sector = u16::from_le_bytes([boot[11], boot[12]]);
    if bytes_per_sector != SD_SECTOR_SIZE as u16 {
        return Err(SdFatError::UnsupportedSectorSize(bytes_per_sector));
    }
    let sectors_per_cluster = parse_sectors_per_cluster(boot)?;
    let reserved_sectors = u16::from_le_bytes([boot[14], boot[15]]) as u32;
    let fats = boot[16];
    if fats == 0 {
        return Err(SdFatError::InvalidBootSector);
    }
    let fat_size = parse_fat_size(boot)?;
    let total_sectors = parse_total_sectors(boot)?;
    let root_cluster = u32::from_le_bytes([boot[44], boot[45], boot[46], boot[47]]);
    if root_cluster < 2 {
        return Err(SdFatError::InvalidBootSector);
    }
    let fat_start_lba = partition_start_lba.saturating_add(reserved_sectors);
    let data_start_lba = fat_start_lba.saturating_add(fat_size.saturating_mul(fats as u32));
    let used_sectors = reserved_sectors.saturating_add(fat_size.saturating_mul(fats as u32));
    if total_sectors <= used_sectors {
        return Err(SdFatError::InvalidBootSector);
    }
    let data_sectors = total_sectors - used_sectors;
    let total_clusters = data_sectors / sectors_per_cluster as u32;
    if total_clusters < 65_525 {
        return Err(SdFatError::UnsupportedFatType);
    }
    Ok(Fat32Volume {
        fat_start_lba,
        fat_size_sectors: fat_size,
        fats,
        data_start_lba,
        sectors_per_cluster,
        root_cluster,
        total_clusters,
    })
}
