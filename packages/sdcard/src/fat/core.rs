use core::cmp;

use crate::probe::{SD_SECTOR_SIZE, SdCardProbe, SdProbeError};

const DIR_ENTRY_SIZE: usize = 32;
const DIR_ENTRIES_PER_SECTOR: usize = SD_SECTOR_SIZE / DIR_ENTRY_SIZE;
const FAT32_EOC: u32 = 0x0FFF_FFF8;
const FAT32_EOC_WRITE: u32 = 0x0FFF_FFFF;
const MAX_PATH_SEGMENTS: usize = 8;
const FAT_NAME_MAX: usize = 96;
const MAX_LFN_SLOTS: usize = 20;
const ATTR_LONG_NAME: u8 = 0x0F;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_VOLUME: u8 = 0x08;

#[derive(Clone, Copy)]
struct PathSegment {
    name: [u8; FAT_NAME_MAX],
    len: u8,
}

impl PathSegment {
    const EMPTY: Self = Self {
        name: [0; FAT_NAME_MAX],
        len: 0,
    };

    fn as_bytes(&self) -> &[u8] {
        &self.name[..self.len as usize]
    }
}

#[derive(Debug)]
pub enum SdFatError {
    Sd(SdProbeError),
    NoFatPartition,
    UnsupportedFatType,
    InvalidBootSector,
    UnsupportedSectorSize(u16),
    UnsupportedSectorsPerCluster(u8),
    InvalidPath,
    PathTooDeep,
    InvalidShortName,
    InvalidLongName,
    NameTooLong,
    NotFound,
    NotDirectory,
    IsDirectory,
    NotEmpty,
    AlreadyExists,
    CrossDirectoryRenameUnsupported,
    DirFull,
    BufferTooSmall { needed: usize },
    NoFreeCluster,
    BadCluster(u32),
    ClusterChainTooLong,
}

impl From<SdProbeError> for SdFatError {
    fn from(value: SdProbeError) -> Self {
        Self::Sd(value)
    }
}

#[derive(Clone, Copy)]
pub struct FatDirEntry {
    pub name: [u8; FAT_NAME_MAX],
    pub name_len: u8,
    pub is_dir: bool,
    pub size: u32,
}

impl FatDirEntry {
    pub const EMPTY: Self = Self {
        name: [0; FAT_NAME_MAX],
        name_len: 0,
        is_dir: false,
        size: 0,
    };
}

#[derive(Clone, Copy)]
struct Fat32Volume {
    fat_start_lba: u32,
    fat_size_sectors: u32,
    fats: u8,
    data_start_lba: u32,
    sectors_per_cluster: u8,
    root_cluster: u32,
    total_clusters: u32,
}

#[derive(Clone, Copy)]
struct DirRecord {
    short_name: [u8; 11],
    display_name: [u8; FAT_NAME_MAX],
    display_name_len: u8,
    attr: u8,
    first_cluster: u32,
    size: u32,
}

impl DirRecord {
    fn is_dir(self) -> bool {
        (self.attr & ATTR_DIRECTORY) != 0
    }
}

#[derive(Clone, Copy)]
struct DirLocation {
    lba: u32,
    slot: u8,
}

impl DirLocation {
    const ZERO: Self = Self { lba: 0, slot: 0 };
}

#[derive(Clone, Copy)]
struct DirFound {
    short_location: DirLocation,
    lfn_locations: [DirLocation; MAX_LFN_SLOTS],
    lfn_count: u8,
    record: DirRecord,
}

#[derive(Clone, Copy)]
struct DirLookup {
    found: Option<DirFound>,
    free: Option<[DirLocation; MAX_LFN_SLOTS + 1]>,
}

#[derive(Clone, Copy)]
struct LfnState {
    expected_slots: u8,
    checksum: u8,
    seen_mask: u8,
    utf16_parts: [[u16; 13]; MAX_LFN_SLOTS],
    lfn_locations: [DirLocation; MAX_LFN_SLOTS],
}

impl LfnState {
    fn new() -> Self {
        Self {
            expected_slots: 0,
            checksum: 0,
            seen_mask: 0,
            utf16_parts: [[0xFFFF; 13]; MAX_LFN_SLOTS],
            lfn_locations: [DirLocation::ZERO; MAX_LFN_SLOTS],
        }
    }

    fn clear(&mut self) {
        *self = Self::new();
    }
}

