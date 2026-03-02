pub async fn append_file(
    sd: &mut SdCardProbe<'_>,
    path: &str,
    data: &[u8],
) -> Result<(), SdFatError> {
    let mut session = begin_append_session(sd, path).await?;
    append_session_write(sd, &mut session, data).await?;
    append_session_flush(sd, &session).await
}

pub struct FatAppendSession {
    volume: Fat32Volume,
    short_location: DirLocation,
    record: DirRecord,
    allocated_clusters: usize,
    tail_cluster: u32,
}

pub async fn begin_append_session(
    sd: &mut SdCardProbe<'_>,
    path: &str,
) -> Result<FatAppendSession, SdFatError> {
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
    append_session_from_record(sd, volume, found.short_location, found.record).await
}

pub async fn begin_append_session_create_or_open(
    sd: &mut SdCardProbe<'_>,
    path: &str,
) -> Result<FatAppendSession, SdFatError> {
    let mut segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let count = parse_path(path, &mut segments)?;
    if count == 0 {
        return Err(SdFatError::InvalidPath);
    }

    let volume = mount_fat32(sd).await?;
    let parent_cluster = resolve_dir_cluster(sd, &volume, &segments, count - 1).await?;
    let target = segments[count - 1];

    if let Some(found) = scan_directory(sd, &volume, parent_cluster, Some(&target), 0)
        .await?
        .found
    {
        if found.record.is_dir() {
            return Err(SdFatError::IsDirectory);
        }
        let mut record = found.record;
        if record.first_cluster >= 2 {
            free_chain(sd, &volume, record.first_cluster).await?;
        }
        record.first_cluster = 0;
        record.size = 0;
        write_directory_entry(sd, &found.short_location, &record).await?;
        return append_session_from_record(sd, volume, found.short_location, record).await;
    }

    let (short_name, lfn_utf16, lfn_len) =
        select_new_entry_name(sd, &volume, parent_cluster, target.as_bytes()).await?;
    let needed_slots = if lfn_len == 0 {
        1usize
    } else {
        ((lfn_len + 12) / 13) + 1
    };
    let free_slots = reserve_directory_slots(sd, &volume, parent_cluster, needed_slots).await?;
    let short_location = free_slots[needed_slots - 1];
    let record = DirRecord {
        short_name,
        display_name: path_segment_to_name(target),
        display_name_len: target.len,
        attr: 0x20,
        first_cluster: 0,
        size: 0,
    };
    write_new_entry(
        sd,
        &free_slots[..needed_slots],
        &record,
        &lfn_utf16[..lfn_len],
    )
    .await?;
    append_session_from_record(sd, volume, short_location, record).await
}

pub async fn append_session_write(
    sd: &mut SdCardProbe<'_>,
    session: &mut FatAppendSession,
    data: &[u8],
) -> Result<(), SdFatError> {
    if data.is_empty() {
        return Ok(());
    }

    let old_size = session.record.size as usize;
    let new_size = old_size
        .checked_add(data.len())
        .ok_or(SdFatError::BufferTooSmall {
            needed: usize::MAX,
        })?;
    let cluster_size = SD_SECTOR_SIZE * session.volume.sectors_per_cluster as usize;
    let new_clusters = clusters_for_size(new_size, cluster_size);
    ensure_append_capacity(sd, session, new_clusters).await?;
    write_data_at(sd, &session.volume, session.record.first_cluster, old_size, data).await?;
    session.record.size = new_size as u32;
    Ok(())
}

pub async fn append_session_reserve(
    sd: &mut SdCardProbe<'_>,
    session: &mut FatAppendSession,
    target_size: usize,
) -> Result<(), SdFatError> {
    let cluster_size = SD_SECTOR_SIZE * session.volume.sectors_per_cluster as usize;
    let target_clusters = clusters_for_size(target_size, cluster_size);
    ensure_append_capacity(sd, session, target_clusters).await
}

pub async fn append_session_flush(
    sd: &mut SdCardProbe<'_>,
    session: &FatAppendSession,
) -> Result<(), SdFatError> {
    write_directory_entry(sd, &session.short_location, &session.record).await
}

async fn ensure_append_capacity(
    sd: &mut SdCardProbe<'_>,
    session: &mut FatAppendSession,
    target_clusters: usize,
) -> Result<(), SdFatError> {
    if target_clusters <= session.allocated_clusters {
        return Ok(());
    }

    if session.allocated_clusters == 0 {
        let first = allocate_chain(sd, &session.volume, target_clusters as u32).await?;
        session.record.first_cluster = first;
        session.tail_cluster = cluster_tail(sd, &session.volume, first, target_clusters).await?;
        session.allocated_clusters = target_clusters;
        return Ok(());
    }

    let extra_clusters = target_clusters - session.allocated_clusters;
    let extra_first = allocate_chain(sd, &session.volume, extra_clusters as u32).await?;
    set_fat_entry(sd, &session.volume, session.tail_cluster, extra_first).await?;
    session.tail_cluster = cluster_tail(sd, &session.volume, extra_first, extra_clusters).await?;
    session.allocated_clusters = target_clusters;
    Ok(())
}

async fn append_session_from_record(
    sd: &mut SdCardProbe<'_>,
    volume: Fat32Volume,
    short_location: DirLocation,
    record: DirRecord,
) -> Result<FatAppendSession, SdFatError> {
    let cluster_size = SD_SECTOR_SIZE * volume.sectors_per_cluster as usize;
    let allocated_clusters = clusters_for_size(record.size as usize, cluster_size);
    let tail_cluster = if allocated_clusters == 0 {
        0
    } else {
        cluster_at_index(sd, &volume, record.first_cluster, allocated_clusters - 1).await?
    };

    Ok(FatAppendSession {
        volume,
        short_location,
        record,
        allocated_clusters,
        tail_cluster,
    })
}

async fn cluster_tail(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    first_cluster: u32,
    cluster_count: usize,
) -> Result<u32, SdFatError> {
    debug_assert!(cluster_count > 0);
    cluster_at_index(sd, volume, first_cluster, cluster_count.saturating_sub(1)).await
}
