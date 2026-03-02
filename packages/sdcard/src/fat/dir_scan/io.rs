async fn reserve_directory_slots(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    dir_cluster: u32,
    needed_slots: usize,
) -> Result<[DirLocation; MAX_LFN_SLOTS + 1], SdFatError> {
    loop {
        let lookup = scan_directory(sd, volume, dir_cluster, None, needed_slots).await?;
        if let Some(free) = lookup.free {
            return Ok(free);
        }
        extend_directory_chain(sd, volume, dir_cluster).await?;
    }
}

async fn extend_directory_chain(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    dir_cluster: u32,
) -> Result<(), SdFatError> {
    let mut tail = dir_cluster;
    let mut visited = 0u32;
    while let Some(next) = next_cluster(sd, volume, tail).await? {
        if visited > volume.total_clusters.saturating_add(2) {
            return Err(SdFatError::ClusterChainTooLong);
        }
        visited = visited.saturating_add(1);
        tail = next;
    }

    let new_cluster = allocate_chain(sd, volume, 1).await?;
    set_fat_entry(sd, volume, tail, new_cluster).await?;

    let first_lba = cluster_to_lba(volume, new_cluster)?;
    let zero = [0u8; SD_SECTOR_SIZE];
    for offset in 0..volume.sectors_per_cluster as u32 {
        sd.write_sector(first_lba + offset, &zero).await?;
    }
    Ok(())
}

async fn write_directory_entry(
    sd: &mut SdCardProbe<'_>,
    location: &DirLocation,
    record: &DirRecord,
) -> Result<(), SdFatError> {
    let mut sector = [0u8; SD_SECTOR_SIZE];
    sd.read_sector(location.lba, &mut sector).await?;

    let base = location.slot as usize * DIR_ENTRY_SIZE;
    for byte in sector[base..base + DIR_ENTRY_SIZE].iter_mut() {
        *byte = 0;
    }
    sector[base..base + 11].copy_from_slice(&record.short_name);
    sector[base + 11] = record.attr;
    let cluster_hi = ((record.first_cluster >> 16) as u16).to_le_bytes();
    let cluster_lo = (record.first_cluster as u16).to_le_bytes();
    sector[base + 20] = cluster_hi[0];
    sector[base + 21] = cluster_hi[1];
    sector[base + 26] = cluster_lo[0];
    sector[base + 27] = cluster_lo[1];
    sector[base + 28..base + 32].copy_from_slice(&record.size.to_le_bytes());

    sd.write_sector(location.lba, &sector).await?;
    Ok(())
}

async fn write_chain_data(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    first_cluster: u32,
    data: &[u8],
) -> Result<(), SdFatError> {
    let mut cluster = first_cluster;
    let mut offset = 0usize;
    let mut visited = 0u32;

    while offset < data.len() {
        if visited > volume.total_clusters.saturating_add(2) {
            return Err(SdFatError::ClusterChainTooLong);
        }
        visited = visited.saturating_add(1);

        for sector_offset in 0..volume.sectors_per_cluster as u32 {
            if offset >= data.len() {
                break;
            }
            let lba = cluster_to_lba(volume, cluster)? + sector_offset;
            let mut sector = [0u8; SD_SECTOR_SIZE];
            let chunk = cmp::min(data.len() - offset, SD_SECTOR_SIZE);
            sector[..chunk].copy_from_slice(&data[offset..offset + chunk]);
            sd.write_sector(lba, &sector).await?;
            offset += chunk;
        }

        if offset >= data.len() {
            break;
        }

        cluster = next_cluster(sd, volume, cluster)
            .await?
            .ok_or(SdFatError::ClusterChainTooLong)?;
    }

    Ok(())
}
