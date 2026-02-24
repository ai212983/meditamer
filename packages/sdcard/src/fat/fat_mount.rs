async fn allocate_chain(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    count: u32,
) -> Result<u32, SdFatError> {
    let mut first = 0u32;
    let mut prev = 0u32;
    let mut search_from = 2u32;

    for _ in 0..count {
        let cluster = find_free_cluster(sd, volume, search_from).await?;
        set_fat_entry(sd, volume, cluster, FAT32_EOC_WRITE).await?;
        if prev != 0 {
            set_fat_entry(sd, volume, prev, cluster).await?;
        } else {
            first = cluster;
        }
        prev = cluster;
        search_from = cluster.saturating_add(1);
    }

    Ok(first)
}

async fn find_free_cluster(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    start_cluster: u32,
) -> Result<u32, SdFatError> {
    let max_cluster = volume.total_clusters.saturating_add(1);
    let start = cmp::max(2, start_cluster);

    for cluster in start..=max_cluster {
        if read_fat_entry(sd, volume, cluster).await? == 0 {
            return Ok(cluster);
        }
    }
    for cluster in 2..start {
        if read_fat_entry(sd, volume, cluster).await? == 0 {
            return Ok(cluster);
        }
    }

    Err(SdFatError::NoFreeCluster)
}

async fn free_chain(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    start_cluster: u32,
) -> Result<(), SdFatError> {
    if start_cluster < 2 {
        return Ok(());
    }

    let max_cluster = volume.total_clusters.saturating_add(1);
    let mut cluster = start_cluster;
    let mut visited = 0u32;

    loop {
        if visited > volume.total_clusters.saturating_add(2) {
            return Err(SdFatError::ClusterChainTooLong);
        }
        visited = visited.saturating_add(1);

        let entry = read_fat_entry(sd, volume, cluster).await?;
        set_fat_entry(sd, volume, cluster, 0).await?;

        if entry >= FAT32_EOC || entry < 2 || entry > max_cluster {
            break;
        }
        cluster = entry;
    }

    Ok(())
}

async fn next_cluster(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    cluster: u32,
) -> Result<Option<u32>, SdFatError> {
    let value = read_fat_entry(sd, volume, cluster).await?;
    if value >= FAT32_EOC {
        return Ok(None);
    }
    if value < 2 || value > volume.total_clusters.saturating_add(1) {
        return Err(SdFatError::BadCluster(value));
    }
    Ok(Some(value))
}

async fn read_fat_entry(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    cluster: u32,
) -> Result<u32, SdFatError> {
    let byte_offset = cluster as u64 * 4;
    let sector_offset = (byte_offset / SD_SECTOR_SIZE as u64) as u32;
    let index = (byte_offset % SD_SECTOR_SIZE as u64) as usize;
    if sector_offset >= volume.fat_size_sectors || index + 4 > SD_SECTOR_SIZE {
        return Err(SdFatError::BadCluster(cluster));
    }

    let lba = volume.fat_start_lba.saturating_add(sector_offset);
    let mut sector = [0u8; SD_SECTOR_SIZE];
    sd.read_sector(lba, &mut sector).await?;
    let raw = u32::from_le_bytes([
        sector[index],
        sector[index + 1],
        sector[index + 2],
        sector[index + 3],
    ]);
    Ok(raw & 0x0FFF_FFFF)
}

async fn set_fat_entry(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    cluster: u32,
    value: u32,
) -> Result<(), SdFatError> {
    let byte_offset = cluster as u64 * 4;
    let sector_offset = (byte_offset / SD_SECTOR_SIZE as u64) as u32;
    let index = (byte_offset % SD_SECTOR_SIZE as u64) as usize;
    if sector_offset >= volume.fat_size_sectors || index + 4 > SD_SECTOR_SIZE {
        return Err(SdFatError::BadCluster(cluster));
    }

    for fat_idx in 0..volume.fats as u32 {
        let fat_base = volume
            .fat_start_lba
            .saturating_add(fat_idx.saturating_mul(volume.fat_size_sectors));
        let lba = fat_base.saturating_add(sector_offset);
        let mut sector = [0u8; SD_SECTOR_SIZE];
        sd.read_sector(lba, &mut sector).await?;
        let old = u32::from_le_bytes([
            sector[index],
            sector[index + 1],
            sector[index + 2],
            sector[index + 3],
        ]);
        let new = (old & 0xF000_0000) | (value & 0x0FFF_FFFF);
        sector[index..index + 4].copy_from_slice(&new.to_le_bytes());
        sd.write_sector(lba, &sector).await?;
    }
    Ok(())
}

fn cluster_to_lba(volume: &Fat32Volume, cluster: u32) -> Result<u32, SdFatError> {
    if cluster < 2 {
        return Err(SdFatError::BadCluster(cluster));
    }
    let index = cluster.saturating_sub(2);
    let lba = volume
        .data_start_lba
        .saturating_add(index.saturating_mul(volume.sectors_per_cluster as u32));
    Ok(lba)
}

async fn mount_fat32(sd: &mut SdCardProbe<'_>) -> Result<Fat32Volume, SdFatError> {
    let mut sector0 = [0u8; SD_SECTOR_SIZE];
    sd.read_sector(0, &mut sector0).await?;

    if let Some(start) = first_fat_partition_lba(&sector0) {
        if let Ok(volume) = parse_fat32_boot(start, sd).await {
            return Ok(volume);
        }
    }

    parse_fat32_boot(0, sd).await
}

async fn parse_fat32_boot(
    partition_start_lba: u32,
    sd: &mut SdCardProbe<'_>,
) -> Result<Fat32Volume, SdFatError> {
    let mut boot = [0u8; SD_SECTOR_SIZE];
    sd.read_sector(partition_start_lba, &mut boot).await?;

    if boot[510] != 0x55 || boot[511] != 0xAA {
        return Err(SdFatError::InvalidBootSector);
    }

    let bytes_per_sector = u16::from_le_bytes([boot[11], boot[12]]);
    if bytes_per_sector != SD_SECTOR_SIZE as u16 {
        return Err(SdFatError::UnsupportedSectorSize(bytes_per_sector));
    }

    let sectors_per_cluster = boot[13];
    if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
        return Err(SdFatError::UnsupportedSectorsPerCluster(sectors_per_cluster));
    }

    let reserved_sectors = u16::from_le_bytes([boot[14], boot[15]]) as u32;
    let fats = boot[16];
    if fats == 0 {
        return Err(SdFatError::InvalidBootSector);
    }

    let fat_size_16 = u16::from_le_bytes([boot[22], boot[23]]) as u32;
    let fat_size_32 = u32::from_le_bytes([boot[36], boot[37], boot[38], boot[39]]);
    let fat_size = if fat_size_16 != 0 {
        fat_size_16
    } else {
        fat_size_32
    };

    if fat_size == 0 || fat_size_32 == 0 {
        return Err(SdFatError::UnsupportedFatType);
    }

    let total_16 = u16::from_le_bytes([boot[19], boot[20]]) as u32;
    let total_32 = u32::from_le_bytes([boot[32], boot[33], boot[34], boot[35]]);
    let total_sectors = if total_16 != 0 { total_16 } else { total_32 };
    if total_sectors == 0 {
        return Err(SdFatError::InvalidBootSector);
    }

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

fn first_fat_partition_lba(sector0: &[u8; SD_SECTOR_SIZE]) -> Option<u32> {
    if sector0[510] != 0x55 || sector0[511] != 0xAA {
        return None;
    }
    for i in 0..4 {
        let base = 446 + i * 16;
        let part_type = sector0[base + 4];
        let is_fat = matches!(part_type, 0x0B | 0x0C | 0x0E | 0x06 | 0x04);
        if !is_fat {
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

