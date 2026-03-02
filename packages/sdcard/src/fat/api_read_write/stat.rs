pub async fn stat(sd: &mut SdCardProbe<'_>, path: &str) -> Result<FatDirEntry, SdFatError> {
    let mut segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let count = parse_path(path, &mut segments)?;
    let volume = mount_fat32(sd).await?;

    if count == 0 {
        let mut name = [0u8; FAT_NAME_MAX];
        name[0] = b'/';
        return Ok(FatDirEntry {
            name,
            name_len: 1,
            is_dir: true,
            size: 0,
        });
    }

    let parent_cluster = resolve_dir_cluster(sd, &volume, &segments, count - 1).await?;
    let found = scan_directory(sd, &volume, parent_cluster, Some(&segments[count - 1]), 0)
        .await?
        .found
        .ok_or(SdFatError::NotFound)?;

    Ok(FatDirEntry {
        name: found.record.display_name,
        name_len: found.record.display_name_len,
        is_dir: found.record.is_dir(),
        size: found.record.size,
    })
}
