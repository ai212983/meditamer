fn clusters_for_size(size: usize, cluster_size: usize) -> usize {
    if size == 0 {
        0
    } else {
        (size + cluster_size - 1) / cluster_size
    }
}

async fn cluster_at_index(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    first_cluster: u32,
    index: usize,
) -> Result<u32, SdFatError> {
    if first_cluster < 2 {
        return Err(SdFatError::BadCluster(first_cluster));
    }
    let mut cluster = first_cluster;
    for _ in 0..index {
        cluster = next_cluster(sd, volume, cluster)
            .await?
            .ok_or(SdFatError::ClusterChainTooLong)?;
    }
    Ok(cluster)
}

async fn write_data_at(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    first_cluster: u32,
    start_offset: usize,
    data: &[u8],
) -> Result<(), SdFatError> {
    if data.is_empty() {
        return Ok(());
    }

    let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
    let cluster_idx = start_offset / cluster_size;
    let mut cluster = cluster_at_index(sd, volume, first_cluster, cluster_idx).await?;
    let mut cluster_offset = start_offset % cluster_size;
    let mut data_idx = 0usize;

    while data_idx < data.len() {
        let sector_start = cluster_offset / SD_SECTOR_SIZE;
        let mut byte_in_sector = cluster_offset % SD_SECTOR_SIZE;

        for sector_off in sector_start..volume.sectors_per_cluster as usize {
            if data_idx >= data.len() {
                break;
            }
            let lba = cluster_to_lba(volume, cluster)? + sector_off as u32;
            let remaining = data.len() - data_idx;
            let write_len = cmp::min(remaining, SD_SECTOR_SIZE - byte_in_sector);
            let mut sector = [0u8; SD_SECTOR_SIZE];

            if byte_in_sector != 0 || write_len < SD_SECTOR_SIZE {
                sd.read_sector(lba, &mut sector).await?;
            }
            sector[byte_in_sector..byte_in_sector + write_len]
                .copy_from_slice(&data[data_idx..data_idx + write_len]);
            sd.write_sector(lba, &sector).await?;
            data_idx += write_len;
            byte_in_sector = 0;
        }

        cluster_offset = 0;
        if data_idx < data.len() {
            cluster = next_cluster(sd, volume, cluster)
                .await?
                .ok_or(SdFatError::ClusterChainTooLong)?;
        }
    }

    Ok(())
}

async fn write_zeroes_at(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    first_cluster: u32,
    start_offset: usize,
    len: usize,
) -> Result<(), SdFatError> {
    let mut remaining = len;
    let zero = [0u8; SD_SECTOR_SIZE];
    let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
    let cluster_idx = start_offset / cluster_size;
    let mut cluster = cluster_at_index(sd, volume, first_cluster, cluster_idx).await?;
    let mut cluster_offset = start_offset % cluster_size;

    while remaining > 0 {
        let sector_start = cluster_offset / SD_SECTOR_SIZE;
        let mut byte_in_sector = cluster_offset % SD_SECTOR_SIZE;
        for sector_off in sector_start..volume.sectors_per_cluster as usize {
            if remaining == 0 {
                break;
            }
            let lba = cluster_to_lba(volume, cluster)? + sector_off as u32;
            let chunk = cmp::min(remaining, SD_SECTOR_SIZE - byte_in_sector);
            if byte_in_sector == 0 && chunk == SD_SECTOR_SIZE {
                sd.write_sector(lba, &zero).await?;
            } else {
                let mut sector = [0u8; SD_SECTOR_SIZE];
                sd.read_sector(lba, &mut sector).await?;
                for byte in sector[byte_in_sector..byte_in_sector + chunk].iter_mut() {
                    *byte = 0;
                }
                sd.write_sector(lba, &sector).await?;
            }
            remaining -= chunk;
            byte_in_sector = 0;
        }
        cluster_offset = 0;
        if remaining > 0 {
            cluster = next_cluster(sd, volume, cluster)
                .await?
                .ok_or(SdFatError::ClusterChainTooLong)?;
        }
    }
    Ok(())
}

async fn zero_tail_after_size(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    first_cluster: u32,
    size: usize,
) -> Result<(), SdFatError> {
    let sector_offset = size % SD_SECTOR_SIZE;
    if sector_offset == 0 {
        return Ok(());
    }
    let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
    let cluster_idx = size / cluster_size;
    let cluster = cluster_at_index(sd, volume, first_cluster, cluster_idx).await?;
    let sector_idx = (size % cluster_size) / SD_SECTOR_SIZE;
    let lba = cluster_to_lba(volume, cluster)? + sector_idx as u32;
    let mut sector = [0u8; SD_SECTOR_SIZE];
    sd.read_sector(lba, &mut sector).await?;
    for byte in sector[sector_offset..].iter_mut() {
        *byte = 0;
    }
    sd.write_sector(lba, &sector).await?;
    Ok(())
}

async fn resolve_dir_cluster(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    path: &[PathSegment; MAX_PATH_SEGMENTS],
    count: usize,
) -> Result<u32, SdFatError> {
    let mut cluster = volume.root_cluster;
    for segment in path.iter().take(count) {
        let lookup = scan_directory(sd, volume, cluster, Some(segment), 0).await?;
        let found = lookup.found.ok_or(SdFatError::NotFound)?;
        let record = found.record;
        if !record.is_dir() {
            return Err(SdFatError::NotDirectory);
        }
        cluster = if record.first_cluster >= 2 {
            record.first_cluster
        } else {
            volume.root_cluster
        };
    }
    Ok(cluster)
}
