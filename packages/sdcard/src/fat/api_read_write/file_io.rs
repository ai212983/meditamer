pub async fn read_file(
    sd: &mut SdCardProbe<'_>,
    path: &str,
    out: &mut [u8],
) -> Result<usize, SdFatError> {
    let mut segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let count = parse_path(path, &mut segments)?;
    if count == 0 {
        return Err(SdFatError::InvalidPath);
    }

    let volume = mount_fat32(sd).await?;
    let parent_cluster = resolve_dir_cluster(sd, &volume, &segments, count - 1).await?;
    let name = &segments[count - 1];

    let lookup = scan_directory(sd, &volume, parent_cluster, Some(name), 0).await?;
    let found = lookup.found.ok_or(SdFatError::NotFound)?;
    let record = found.record;
    if record.is_dir() {
        return Err(SdFatError::IsDirectory);
    }

    let file_size = record.size as usize;
    if out.len() < file_size {
        return Err(SdFatError::BufferTooSmall { needed: file_size });
    }
    if file_size == 0 {
        return Ok(0);
    }
    if record.first_cluster < 2 {
        return Err(SdFatError::BadCluster(record.first_cluster));
    }

    let mut remaining = file_size;
    let mut written = 0usize;
    let mut cluster = record.first_cluster;
    let mut visited = 0u32;

    while remaining > 0 {
        if visited > volume.total_clusters.saturating_add(2) {
            return Err(SdFatError::ClusterChainTooLong);
        }
        visited = visited.saturating_add(1);

        for sector_offset in 0..volume.sectors_per_cluster as u32 {
            if remaining == 0 {
                break;
            }
            let lba = cluster_to_lba(&volume, cluster)? + sector_offset;
            let mut sector = [0u8; SD_SECTOR_SIZE];
            sd.read_sector(lba, &mut sector).await?;

            let chunk = cmp::min(remaining, SD_SECTOR_SIZE);
            out[written..written + chunk].copy_from_slice(&sector[..chunk]);
            written += chunk;
            remaining -= chunk;
        }

        if remaining == 0 {
            break;
        }

        cluster = next_cluster(sd, &volume, cluster)
            .await?
            .ok_or(SdFatError::ClusterChainTooLong)?;
    }

    Ok(written)
}

pub async fn write_file(
    sd: &mut SdCardProbe<'_>,
    path: &str,
    data: &[u8],
) -> Result<(), SdFatError> {
    let mut segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let count = parse_path(path, &mut segments)?;
    if count == 0 {
        return Err(SdFatError::InvalidPath);
    }

    let volume = mount_fat32(sd).await?;
    let parent_cluster = resolve_dir_cluster(sd, &volume, &segments, count - 1).await?;
    let target_name = segments[count - 1];
    let lookup = scan_directory(
        sd,
        &volume,
        parent_cluster,
        Some(&target_name),
        MAX_LFN_SLOTS + 1,
    )
    .await?;

    if let Some(found) = lookup.found {
        if found.record.is_dir() {
            return Err(SdFatError::IsDirectory);
        }
    }

    let old_first_cluster = lookup
        .found
        .map(|found| found.record.first_cluster)
        .unwrap_or(0);
    if old_first_cluster >= 2 {
        free_chain(sd, &volume, old_first_cluster).await?;
    }

    let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
    let clusters_needed = if data.is_empty() {
        0
    } else {
        (data.len() + cluster_size - 1) / cluster_size
    };
    let new_first_cluster = if clusters_needed == 0 {
        0
    } else {
        allocate_chain(sd, &volume, clusters_needed as u32).await?
    };

    if clusters_needed > 0 {
        write_chain_data(sd, &volume, new_first_cluster, data).await?;
    }

    if let Some(found) = lookup.found {
        let mut record = found.record;
        record.first_cluster = new_first_cluster;
        record.size = data.len() as u32;
        write_directory_entry(sd, &found.short_location, &record).await?;
        return Ok(());
    }

    let (short_name, lfn_utf16, lfn_len) =
        select_new_entry_name(sd, &volume, parent_cluster, target_name.as_bytes()).await?;
    let needed_slots = if lfn_len == 0 {
        1usize
    } else {
        ((lfn_len + 12) / 13) + 1
    };
    let free_slots = reserve_directory_slots(sd, &volume, parent_cluster, needed_slots).await?;
    write_new_entry(
        sd,
        &free_slots[..needed_slots],
        &DirRecord {
            short_name,
            display_name: path_segment_to_name(target_name),
            display_name_len: target_name.len,
            attr: 0x20,
            first_cluster: new_first_cluster,
            size: data.len() as u32,
        },
        &lfn_utf16[..lfn_len],
    )
    .await?;

    Ok(())
}
