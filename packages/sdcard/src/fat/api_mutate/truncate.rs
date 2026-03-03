pub async fn truncate_file(
    sd: &mut SdCardProbe<'_>,
    path: &str,
    new_size: usize,
) -> Result<(), SdFatError> {
    let mut segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let count = parse_path(path, &mut segments)?;
    if count == 0 {
        return Err(SdFatError::InvalidPath);
    }

    let volume = mount_fat32(sd).await?;
    let parent_cluster = resolve_dir_cluster(sd, &volume, &segments, count - 1).await?;
    let found = scan_directory(sd, &volume, parent_cluster, Some(&segments[count - 1]), 0)
        .await?
        .found
        .ok_or(SdFatError::NotFound)?;
    if found.record.is_dir() {
        return Err(SdFatError::IsDirectory);
    }

    let old_size = found.record.size as usize;
    if new_size == old_size {
        return Ok(());
    }

    let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
    let old_clusters = clusters_for_size(old_size, cluster_size);
    let target_clusters = clusters_for_size(new_size, cluster_size);
    let mut first_cluster = found.record.first_cluster;

    if target_clusters == 0 {
        if first_cluster >= 2 {
            free_chain_for_record(sd, &volume, first_cluster, found.record.size).await?;
        }
        first_cluster = 0;
    } else if old_clusters == 0 {
        first_cluster = allocate_chain(sd, &volume, target_clusters as u32).await?;
    } else if target_clusters > old_clusters {
        let extra = allocate_chain(sd, &volume, (target_clusters - old_clusters) as u32).await?;
        let tail = cluster_at_index(sd, &volume, first_cluster, old_clusters - 1).await?;
        set_fat_entry(sd, &volume, tail, extra).await?;
    } else if target_clusters < old_clusters {
        let keep_tail = cluster_at_index(sd, &volume, first_cluster, target_clusters - 1).await?;
        let free_start = next_cluster(sd, &volume, keep_tail).await?;
        set_fat_entry(sd, &volume, keep_tail, FAT32_EOC_WRITE).await?;
        if let Some(start) = free_start {
            free_chain_for_expected_clusters(sd, &volume, start, old_clusters - target_clusters)
                .await?;
        }
    }

    if new_size > old_size {
        write_zeroes_at(sd, &volume, first_cluster, old_size, new_size - old_size).await?;
    } else if new_size > 0 {
        zero_tail_after_size(sd, &volume, first_cluster, new_size).await?;
    }

    let mut record = found.record;
    record.first_cluster = first_cluster;
    record.size = new_size as u32;
    write_directory_entry(sd, &found.short_location, &record).await
}
