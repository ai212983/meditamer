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
