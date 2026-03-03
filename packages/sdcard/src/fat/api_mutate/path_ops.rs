pub async fn mkdir(sd: &mut SdCardProbe<'_>, path: &str) -> Result<(), SdFatError> {
    let mut segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let count = parse_path(path, &mut segments)?;
    if count == 0 {
        return Err(SdFatError::InvalidPath);
    }

    let volume = mount_fat32(sd).await?;
    let parent_cluster = resolve_dir_cluster(sd, &volume, &segments, count - 1).await?;
    let target = segments[count - 1];
    if let Ok(short_target) = encode_short_name(target.as_bytes()) {
        if short_name_exists(sd, &volume, parent_cluster, &short_target).await? {
            return Err(SdFatError::AlreadyExists);
        }
    }
    let existing = scan_directory(sd, &volume, parent_cluster, Some(&target), 0).await?;
    if existing.found.is_some() {
        return Err(SdFatError::AlreadyExists);
    }

    let (short_name, lfn_utf16, lfn_len) =
        select_new_entry_name(sd, &volume, parent_cluster, target.as_bytes()).await?;
    let needed_slots = if lfn_len == 0 {
        1usize
    } else {
        ((lfn_len + 12) / 13) + 1
    };
    let free_slots = reserve_directory_slots(sd, &volume, parent_cluster, needed_slots).await?;
    let dir_cluster = allocate_chain(sd, &volume, 1).await?;
    initialize_directory_cluster(sd, &volume, dir_cluster, parent_cluster).await?;

    if let Err(err) = write_new_entry(
        sd,
        &free_slots[..needed_slots],
        &DirRecord {
            short_name,
            display_name: path_segment_to_name(target),
            display_name_len: target.len,
            attr: ATTR_DIRECTORY,
            first_cluster: dir_cluster,
            size: 0,
        },
        &lfn_utf16[..lfn_len],
    )
    .await
    {
        let _ = free_chain(sd, &volume, dir_cluster).await;
        return Err(err);
    }
    Ok(())
}

pub async fn remove(sd: &mut SdCardProbe<'_>, path: &str) -> Result<(), SdFatError> {
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
        if !is_directory_empty(sd, &volume, found.record.first_cluster).await? {
            return Err(SdFatError::NotEmpty);
        }
    }
    if found.record.first_cluster >= 2 {
        if found.record.is_dir() {
            free_chain(sd, &volume, found.record.first_cluster).await?;
        } else {
            free_chain_for_record(sd, &volume, found.record.first_cluster, found.record.size)
                .await?;
        }
    }
    mark_found_deleted(sd, &found).await
}

pub async fn rename(sd: &mut SdCardProbe<'_>, src: &str, dst: &str) -> Result<(), SdFatError> {
    let mut src_segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let src_count = parse_path(src, &mut src_segments)?;
    let mut dst_segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let dst_count = parse_path(dst, &mut dst_segments)?;
    if src_count == 0 || dst_count == 0 {
        return Err(SdFatError::InvalidPath);
    }

    let volume = mount_fat32(sd).await?;
    let src_parent = resolve_dir_cluster(sd, &volume, &src_segments, src_count - 1).await?;
    let dst_parent = resolve_dir_cluster(sd, &volume, &dst_segments, dst_count - 1).await?;
    let src_found = scan_directory(sd, &volume, src_parent, Some(&src_segments[src_count - 1]), 0)
        .await?
        .found
        .ok_or(SdFatError::NotFound)?;
    if src_found.record.is_dir() && src_parent != dst_parent {
        return Err(SdFatError::CrossDirectoryRenameUnsupported);
    }
    if scan_directory(sd, &volume, dst_parent, Some(&dst_segments[dst_count - 1]), 0)
        .await?
        .found
        .is_some()
    {
        return Err(SdFatError::AlreadyExists);
    }

    let dst_name = dst_segments[dst_count - 1];
    let (short_name, lfn_utf16, lfn_len) =
        select_new_entry_name(sd, &volume, dst_parent, dst_name.as_bytes()).await?;
    let needed_slots = if lfn_len == 0 {
        1usize
    } else {
        ((lfn_len + 12) / 13) + 1
    };
    let free_slots = reserve_directory_slots(sd, &volume, dst_parent, needed_slots).await?;

    write_new_entry(
        sd,
        &free_slots[..needed_slots],
        &DirRecord {
            short_name,
            display_name: path_segment_to_name(dst_name),
            display_name_len: dst_name.len,
            attr: src_found.record.attr,
            first_cluster: src_found.record.first_cluster,
            size: src_found.record.size,
        },
        &lfn_utf16[..lfn_len],
    )
    .await?;
    mark_found_deleted(sd, &src_found).await
}

pub async fn rename_replace(
    sd: &mut SdCardProbe<'_>,
    src: &str,
    dst: &str,
) -> Result<(), SdFatError> {
    let mut src_segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let src_count = parse_path(src, &mut src_segments)?;
    let mut dst_segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let dst_count = parse_path(dst, &mut dst_segments)?;
    if src_count == 0 || dst_count == 0 {
        return Err(SdFatError::InvalidPath);
    }

    let volume = mount_fat32(sd).await?;
    let src_parent = resolve_dir_cluster(sd, &volume, &src_segments, src_count - 1).await?;
    let dst_parent = resolve_dir_cluster(sd, &volume, &dst_segments, dst_count - 1).await?;
    let src_found = scan_directory(sd, &volume, src_parent, Some(&src_segments[src_count - 1]), 0)
        .await?
        .found
        .ok_or(SdFatError::NotFound)?;

    if src_found.record.is_dir() && src_parent != dst_parent {
        return Err(SdFatError::CrossDirectoryRenameUnsupported);
    }

    if let Some(dst_found) = scan_directory(sd, &volume, dst_parent, Some(&dst_segments[dst_count - 1]), 0)
        .await?
        .found
    {
        if dst_found.record.is_dir() {
            return Err(SdFatError::IsDirectory);
        }
        if dst_found.record.first_cluster >= 2 {
            free_chain_for_record(
                sd,
                &volume,
                dst_found.record.first_cluster,
                dst_found.record.size,
            )
            .await?;
        }
        mark_found_deleted(sd, &dst_found).await?;
    }

    let dst_name = dst_segments[dst_count - 1];
    let (short_name, lfn_utf16, lfn_len) =
        select_new_entry_name(sd, &volume, dst_parent, dst_name.as_bytes()).await?;
    let needed_slots = if lfn_len == 0 {
        1usize
    } else {
        ((lfn_len + 12) / 13) + 1
    };
    let free_slots = reserve_directory_slots(sd, &volume, dst_parent, needed_slots).await?;

    write_new_entry(
        sd,
        &free_slots[..needed_slots],
        &DirRecord {
            short_name,
            display_name: path_segment_to_name(dst_name),
            display_name_len: dst_name.len,
            attr: src_found.record.attr,
            first_cluster: src_found.record.first_cluster,
            size: src_found.record.size,
        },
        &lfn_utf16[..lfn_len],
    )
    .await?;
    mark_found_deleted(sd, &src_found).await
}
