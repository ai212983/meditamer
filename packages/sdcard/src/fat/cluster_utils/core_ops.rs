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
