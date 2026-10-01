use std::collections::BTreeMap;

use sdcard::fat::{FatEngine, FatIoAction, FatIoCompletion, FatRequest, FatResult, FatStep};
use sdcard::probe::SD_SECTOR_SIZE;

pub const FAT_START: u32 = 1;
pub const FAT_SECTORS: u32 = 512;
pub const DATA_START: u32 = FAT_START + FAT_SECTORS;
pub const ROOT_CLUSTER: u32 = 2;

pub struct FakeDisk {
    pub sectors: BTreeMap<u32, [u8; SD_SECTOR_SIZE]>,
    pub trace: Vec<FatIoAction>,
}

impl FakeDisk {
    pub fn fat32() -> Self {
        let mut disk = Self {
            sectors: BTreeMap::new(),
            trace: Vec::new(),
        };
        let mut boot = [0u8; SD_SECTOR_SIZE];
        boot[11..13].copy_from_slice(&(SD_SECTOR_SIZE as u16).to_le_bytes());
        boot[13] = 1;
        boot[14..16].copy_from_slice(&1u16.to_le_bytes());
        boot[16] = 1;
        boot[32..36].copy_from_slice(&66_038u32.to_le_bytes());
        boot[36..40].copy_from_slice(&FAT_SECTORS.to_le_bytes());
        boot[44..48].copy_from_slice(&ROOT_CLUSTER.to_le_bytes());
        boot[510] = 0x55;
        boot[511] = 0xAA;
        disk.sectors.insert(0, boot);
        disk.set_fat(0, 0x0FFF_FFF8);
        disk.set_fat(1, 0x0FFF_FFFF);
        disk.set_fat(ROOT_CLUSTER, 0x0FFF_FFFF);
        disk
    }

    /// Valid FAT32 fixture with configurable `sectors_per_cluster`.
    /// `fat32()` stays byte-identical; this shares its geometry with
    /// `total_sectors = reserved + fat + 65_525 * spc` so the volume still
    /// mounts as FAT32 for any power-of-two `spc`.
    pub fn fat32_with_spc(sectors_per_cluster: u8) -> Self {
        assert!(sectors_per_cluster.is_power_of_two());
        let total_sectors = 1 + FAT_SECTORS + 65_525u32 * u32::from(sectors_per_cluster);
        let mut disk = Self {
            sectors: BTreeMap::new(),
            trace: Vec::new(),
        };
        let mut boot = [0u8; SD_SECTOR_SIZE];
        boot[11..13].copy_from_slice(&(SD_SECTOR_SIZE as u16).to_le_bytes());
        boot[13] = sectors_per_cluster;
        boot[14..16].copy_from_slice(&1u16.to_le_bytes());
        boot[16] = 1;
        boot[32..36].copy_from_slice(&total_sectors.to_le_bytes());
        boot[36..40].copy_from_slice(&FAT_SECTORS.to_le_bytes());
        boot[44..48].copy_from_slice(&ROOT_CLUSTER.to_le_bytes());
        boot[510] = 0x55;
        boot[511] = 0xAA;
        disk.sectors.insert(0, boot);
        disk.set_fat(0, 0x0FFF_FFF8);
        disk.set_fat(1, 0x0FFF_FFFF);
        disk.set_fat(ROOT_CLUSTER, 0x0FFF_FFFF);
        disk
    }

    pub fn set_fat(&mut self, cluster: u32, value: u32) {
        let offset = cluster as usize * 4;
        let lba = FAT_START + (offset / SD_SECTOR_SIZE) as u32;
        let index = offset % SD_SECTOR_SIZE;
        self.sectors.entry(lba).or_insert([0; SD_SECTOR_SIZE])[index..index + 4]
            .copy_from_slice(&value.to_le_bytes());
    }

    pub fn fat(&self, cluster: u32) -> u32 {
        let offset = cluster as usize * 4;
        let lba = FAT_START + (offset / SD_SECTOR_SIZE) as u32;
        let index = offset % SD_SECTOR_SIZE;
        u32::from_le_bytes(self.read(lba)[index..index + 4].try_into().unwrap()) & 0x0FFF_FFFF
    }

    pub fn read(&self, lba: u32) -> [u8; SD_SECTOR_SIZE] {
        self.sectors
            .get(&lba)
            .copied()
            .unwrap_or([0; SD_SECTOR_SIZE])
    }

    pub fn execute(
        &mut self,
        action: FatIoAction,
        engine: &mut FatEngine,
        input: &[u8],
        output: &mut [u8],
    ) {
        self.trace.push(action);
        match action {
            FatIoAction::ReadSector { lba, .. } => {
                engine.workspace_mut().sector = self.read(lba);
            }
            FatIoAction::WriteSector { lba, .. } => {
                self.sectors.insert(lba, engine.workspace().sector);
            }
            FatIoAction::ReadSectorToPayload {
                lba,
                payload_offset,
                sector_offset,
                len,
                ..
            } => {
                engine.workspace_mut().sector = self.read(lba);
                let start = payload_offset as usize;
                let source = sector_offset as usize;
                output[start..start + len as usize]
                    .copy_from_slice(&engine.workspace().sector[source..source + len as usize]);
            }
            FatIoAction::WriteSectorFromPayload {
                lba,
                payload_offset,
                sector_offset,
                len,
                preserve_existing,
                ..
            } => {
                if !preserve_existing {
                    engine.workspace_mut().sector.fill(0);
                }
                let src = payload_offset as usize;
                let dst = sector_offset as usize;
                engine.workspace_mut().sector[dst..dst + len as usize]
                    .copy_from_slice(&input[src..src + len as usize]);
                self.sectors.insert(lba, engine.workspace().sector);
            }
            FatIoAction::WritePayloadSectors {
                start_lba,
                payload_offset,
                sectors,
                ..
            } => {
                for sector in 0..u32::from(sectors) {
                    let start = payload_offset as usize + sector as usize * SD_SECTOR_SIZE;
                    let mut data = [0u8; SD_SECTOR_SIZE];
                    data.copy_from_slice(&input[start..start + SD_SECTOR_SIZE]);
                    self.sectors.insert(start_lba + sector, data);
                }
            }
        }
    }
}

pub fn path(value: &str) -> ([u8; sdcard::SD_PATH_MAX], u8) {
    let mut out = [0u8; sdcard::SD_PATH_MAX];
    out[..value.len()].copy_from_slice(value.as_bytes());
    (out, value.len() as u8)
}

pub fn run(
    disk: &mut FakeDisk,
    engine: &mut FatEngine,
    request: FatRequest,
    input: &[u8],
    output: &mut [u8],
) -> FatResult {
    engine.start(request).unwrap();
    let mut completion = FatIoCompletion::Pending;
    for _ in 0..200_000 {
        match engine.advance(completion) {
            FatStep::Io(action) => {
                assert!(engine.has_outstanding_io());
                disk.execute(action, engine, input, output);
                completion = FatIoCompletion::Done;
            }
            FatStep::Continue | FatStep::Yield => completion = FatIoCompletion::Pending,
            FatStep::Complete(result) => return result,
        }
    }
    panic!("engine failed to complete");
}
