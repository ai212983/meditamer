pub async fn list_dir(
    sd: &mut SdCardProbe<'_>,
    path: &str,
    out: &mut [FatDirEntry],
) -> Result<usize, SdFatError> {
    let mut segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
    let count = parse_path(path, &mut segments)?;
    let volume = mount_fat32(sd).await?;
    let dir_cluster = resolve_dir_cluster(sd, &volume, &segments, count).await?;

    let mut entries_written = 0usize;
    let mut cluster = dir_cluster;
    let mut visited = 0u32;
    let mut lfn = LfnState::new();

    loop {
        mark_cluster_visited(&mut visited, volume.total_clusters)?;
        let cluster_outcome =
            collect_cluster_entries(sd, &volume, cluster, out, &mut entries_written, &mut lfn)
                .await?;
        if matches!(cluster_outcome, ListDirClusterOutcome::Done) {
            return Ok(entries_written);
        }

        match next_cluster(sd, &volume, cluster).await? {
            Some(next) => cluster = next,
            None => return Ok(entries_written),
        }
    }
}

fn mark_cluster_visited(visited: &mut u32, total_clusters: u32) -> Result<(), SdFatError> {
    if *visited > total_clusters.saturating_add(2) {
        return Err(SdFatError::ClusterChainTooLong);
    }
    *visited = visited.saturating_add(1);
    Ok(())
}

enum ListDirClusterOutcome {
    Done,
    Continue,
}

async fn collect_cluster_entries(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    cluster: u32,
    out: &mut [FatDirEntry],
    entries_written: &mut usize,
    lfn: &mut LfnState,
) -> Result<ListDirClusterOutcome, SdFatError> {
    for sector_offset in 0..volume.sectors_per_cluster as u32 {
        let lba = cluster_to_lba(volume, cluster)? + sector_offset;
        let mut sector = [0u8; SD_SECTOR_SIZE];
        sd.read_sector(lba, &mut sector).await?;

        for slot in 0..DIR_ENTRIES_PER_SECTOR {
            match parse_list_dir_slot(&sector, lba, slot as u8, lfn) {
                ListDirSlot::Continue => {}
                ListDirSlot::End => return Ok(ListDirClusterOutcome::Done),
                ListDirSlot::Entry(entry) => {
                    if *entries_written >= out.len() {
                        return Ok(ListDirClusterOutcome::Done);
                    }
                    out[*entries_written] = entry;
                    *entries_written += 1;
                }
            }
        }
    }

    Ok(ListDirClusterOutcome::Continue)
}

enum ListDirSlot {
    Continue,
    End,
    Entry(FatDirEntry),
}

fn parse_list_dir_slot(
    sector: &[u8; SD_SECTOR_SIZE],
    lba: u32,
    slot: u8,
    lfn: &mut LfnState,
) -> ListDirSlot {
    let base = slot as usize * DIR_ENTRY_SIZE;
    let first = sector[base];
    if first == 0x00 {
        return ListDirSlot::End;
    }
    if first == 0xE5 {
        lfn.clear();
        return ListDirSlot::Continue;
    }

    let attr = sector[base + 11];
    if attr == ATTR_LONG_NAME {
        consume_lfn_entry(lfn, DirLocation { lba, slot }, &sector[base..base + DIR_ENTRY_SIZE]);
        return ListDirSlot::Continue;
    }
    if (attr & ATTR_VOLUME) != 0 {
        lfn.clear();
        return ListDirSlot::Continue;
    }

    let mut raw_name = [0u8; 11];
    raw_name.copy_from_slice(&sector[base..base + 11]);
    let mut name = [0u8; FAT_NAME_MAX];
    let (name_len, _) = build_display_name_into(lfn, &raw_name, &mut name);
    let size = u32::from_le_bytes([
        sector[base + 28],
        sector[base + 29],
        sector[base + 30],
        sector[base + 31],
    ]);
    lfn.clear();

    ListDirSlot::Entry(FatDirEntry {
        name,
        name_len: name_len as u8,
        is_dir: (attr & ATTR_DIRECTORY) != 0,
        size,
    })
}
