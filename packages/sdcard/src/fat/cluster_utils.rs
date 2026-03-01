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

    'cluster_loop: while data_idx < data.len() {
        let sector_start = cluster_offset / SD_SECTOR_SIZE;
        let mut byte_in_sector = cluster_offset % SD_SECTOR_SIZE;
        let mut sector_off = sector_start;
        while sector_off < volume.sectors_per_cluster as usize {
            if data_idx >= data.len() {
                break;
            }
            let lba = cluster_to_lba(volume, cluster)? + sector_off as u32;
            if let Some((next_cluster_value, next_cluster_offset, next_data_idx)) =
                try_write_contiguous_burst(
                    sd,
                    volume,
                    cluster,
                    sector_off,
                    lba,
                    byte_in_sector,
                    data,
                    data_idx,
                )
                .await?
            {
                cluster = next_cluster_value;
                cluster_offset = next_cluster_offset;
                data_idx = next_data_idx;
                continue 'cluster_loop;
            }

            let write_len =
                write_data_sector_chunk(sd, lba, byte_in_sector, &data[data_idx..]).await?;
            data_idx += write_len;
            byte_in_sector = 0;
            sector_off += 1;
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

async fn try_write_contiguous_burst(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    cluster: u32,
    sector_off: usize,
    lba: u32,
    byte_in_sector: usize,
    data: &[u8],
    data_idx: usize,
) -> Result<Option<(u32, usize, usize)>, SdFatError> {
    if byte_in_sector != 0 {
        return Ok(None);
    }

    let remaining = data.len().saturating_sub(data_idx);
    let full_sectors = remaining / SD_SECTOR_SIZE;
    if full_sectors < 2 {
        return Ok(None);
    }

    // Opportunistically extend full-sector bursts across physically contiguous
    // FAT clusters (n, n+1, ...). This keeps correctness under fragmentation
    // while unlocking multi-block throughput on contiguous files.
    let contiguous_sectors =
        contiguous_full_sector_run(sd, volume, cluster, sector_off, full_sectors).await?;
    if contiguous_sectors < 2 {
        return Ok(None);
    }

    let write_bytes = contiguous_sectors * SD_SECTOR_SIZE;
    sd.write_sectors_contiguous(lba, &data[data_idx..data_idx + write_bytes])
        .await?;

    let next_data_idx = data_idx + write_bytes;
    let (mut next_cluster_value, next_sector_off) =
        advance_cluster_sector_position(sd, volume, cluster, sector_off, contiguous_sectors).await?;

    if next_sector_off == 0 && next_data_idx < data.len() {
        next_cluster_value = next_cluster(sd, volume, next_cluster_value)
            .await?
            .ok_or(SdFatError::ClusterChainTooLong)?;
    }

    Ok(Some((
        next_cluster_value,
        next_sector_off * SD_SECTOR_SIZE,
        next_data_idx,
    )))
}

async fn write_data_sector_chunk(
    sd: &mut SdCardProbe<'_>,
    lba: u32,
    byte_in_sector: usize,
    src: &[u8],
) -> Result<usize, SdFatError> {
    let write_len = cmp::min(src.len(), SD_SECTOR_SIZE - byte_in_sector);
    let mut sector = [0u8; SD_SECTOR_SIZE];
    if byte_in_sector != 0 || write_len < SD_SECTOR_SIZE {
        sd.read_sector(lba, &mut sector).await?;
    }
    sector[byte_in_sector..byte_in_sector + write_len].copy_from_slice(&src[..write_len]);
    sd.write_sector(lba, &sector).await?;
    Ok(write_len)
}

async fn contiguous_full_sector_run(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    start_cluster: u32,
    start_sector_off: usize,
    max_sectors: usize,
) -> Result<usize, SdFatError> {
    if max_sectors == 0 {
        return Ok(0);
    }

    let sectors_per_cluster = volume.sectors_per_cluster as usize;
    if start_sector_off >= sectors_per_cluster {
        return Ok(0);
    }

    let mut run_sectors = cmp::min(max_sectors, sectors_per_cluster - start_sector_off);
    if run_sectors == 0 || run_sectors == max_sectors || start_sector_off + run_sectors < sectors_per_cluster
    {
        return Ok(run_sectors);
    }

    let mut cluster = start_cluster;
    while run_sectors < max_sectors {
        let Some(next) = next_cluster(sd, volume, cluster).await? else {
            break;
        };
        if next != cluster.saturating_add(1) {
            break;
        }
        cluster = next;
        let remaining = max_sectors - run_sectors;
        let extend = cmp::min(remaining, sectors_per_cluster);
        run_sectors += extend;
        if extend < sectors_per_cluster {
            break;
        }
    }

    Ok(run_sectors)
}

async fn advance_cluster_sector_position(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    start_cluster: u32,
    start_sector_off: usize,
    sectors_to_advance: usize,
) -> Result<(u32, usize), SdFatError> {
    let sectors_per_cluster = volume.sectors_per_cluster as usize;
    let mut cluster = start_cluster;
    let mut sector_off = start_sector_off;
    let mut remaining = sectors_to_advance;

    while remaining > 0 {
        let sectors_left = sectors_per_cluster - sector_off;
        if remaining < sectors_left {
            sector_off += remaining;
            break;
        }
        remaining -= sectors_left;
        sector_off = 0;
        if remaining > 0 {
            cluster = next_cluster(sd, volume, cluster)
                .await?
                .ok_or(SdFatError::ClusterChainTooLong)?;
        }
    }

    Ok((cluster, sector_off))
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
            zero_sector_chunk(sd, lba, byte_in_sector, chunk, &zero).await?;
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

async fn zero_sector_chunk(
    sd: &mut SdCardProbe<'_>,
    lba: u32,
    byte_in_sector: usize,
    chunk: usize,
    zero: &[u8; SD_SECTOR_SIZE],
) -> Result<(), SdFatError> {
    if byte_in_sector == 0 && chunk == SD_SECTOR_SIZE {
        sd.write_sector(lba, zero).await?;
        return Ok(());
    }

    let mut sector = [0u8; SD_SECTOR_SIZE];
    sd.read_sector(lba, &mut sector).await?;
    for byte in sector[byte_in_sector..byte_in_sector + chunk].iter_mut() {
        *byte = 0;
    }
    sd.write_sector(lba, &sector).await?;
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
