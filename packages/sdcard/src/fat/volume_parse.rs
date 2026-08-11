use crate::{
    fat::{Fat32Volume, SdFatError},
    probe::SD_SECTOR_SIZE,
};

pub(super) fn cluster_to_lba(volume: &Fat32Volume, cluster: u32) -> Result<u32, SdFatError> {
    if cluster < 2 {
        return Err(SdFatError::BadCluster(cluster));
    }
    Ok(volume.data_start_lba.saturating_add(
        cluster
            .saturating_sub(2)
            .saturating_mul(volume.sectors_per_cluster as u32),
    ))
}

pub(super) fn parse_sectors_per_cluster(boot: &[u8; SD_SECTOR_SIZE]) -> Result<u8, SdFatError> {
    let value = boot[13];
    if value == 0 || !value.is_power_of_two() {
        return Err(SdFatError::UnsupportedSectorsPerCluster(value));
    }
    Ok(value)
}

pub(super) fn parse_fat_size(boot: &[u8; SD_SECTOR_SIZE]) -> Result<u32, SdFatError> {
    let fat16 = u16::from_le_bytes([boot[22], boot[23]]) as u32;
    let fat32 = u32::from_le_bytes([boot[36], boot[37], boot[38], boot[39]]);
    let value = if fat16 != 0 { fat16 } else { fat32 };
    if value == 0 || fat32 == 0 {
        return Err(SdFatError::UnsupportedFatType);
    }
    Ok(value)
}

pub(super) fn parse_total_sectors(boot: &[u8; SD_SECTOR_SIZE]) -> Result<u32, SdFatError> {
    let total16 = u16::from_le_bytes([boot[19], boot[20]]) as u32;
    let total32 = u32::from_le_bytes([boot[32], boot[33], boot[34], boot[35]]);
    let value = if total16 != 0 { total16 } else { total32 };
    if value == 0 {
        return Err(SdFatError::InvalidBootSector);
    }
    Ok(value)
}

pub(super) fn first_fat_partition_lba(sector0: &[u8; SD_SECTOR_SIZE]) -> Option<u32> {
    if sector0[510] != 0x55 || sector0[511] != 0xAA {
        return None;
    }
    for index in 0..4 {
        let base = 446 + index * 16;
        if !matches!(sector0[base + 4], 0x0B | 0x0C | 0x0E | 0x06 | 0x04) {
            continue;
        }
        let start = u32::from_le_bytes([
            sector0[base + 8],
            sector0[base + 9],
            sector0[base + 10],
            sector0[base + 11],
        ]);
        if start != 0 {
            return Some(start);
        }
    }
    None
}
