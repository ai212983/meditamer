async fn scan_directory(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    dir_cluster: u32,
    target_name: Option<&PathSegment>,
    needed_free_slots: usize,
) -> Result<DirLookup, SdFatError> {
    let mut cluster = dir_cluster;
    let mut free_slots = None;
    let mut free_run = [DirLocation::ZERO; MAX_LFN_SLOTS + 1];
    let mut free_run_len = 0usize;
    let mut reached_directory_end = false;
    let mut visited = 0u32;
    let mut lfn = LfnState::new();

    loop {
        if visited > volume.total_clusters.saturating_add(2) {
            return Err(SdFatError::ClusterChainTooLong);
        }
        visited = visited.saturating_add(1);

        for sector_offset in 0..volume.sectors_per_cluster as u32 {
            let lba = cluster_to_lba(volume, cluster)? + sector_offset;
            let mut sector = [0u8; SD_SECTOR_SIZE];
            sd.read_sector(lba, &mut sector).await?;

            for slot in 0..DIR_ENTRIES_PER_SECTOR {
                let base = slot * DIR_ENTRY_SIZE;
                let first = sector[base];
                let (is_free_slot, next_reached_directory_end) =
                    classify_directory_slot(first, reached_directory_end);
                reached_directory_end = next_reached_directory_end;
                if is_free_slot {
                    lfn.clear();
                    if needed_free_slots > 0 {
                        record_free_slot(
                            &mut free_run,
                            &mut free_run_len,
                            needed_free_slots,
                            &mut free_slots,
                            DirLocation {
                                lba,
                                slot: slot as u8,
                            },
                        );
                        if free_slots.is_some() && target_name.is_none() {
                            return Ok(DirLookup {
                                found: None,
                                free: free_slots,
                            });
                        }
                    }
                    continue;
                }

                let attr = sector[base + 11];
                if attr == ATTR_LONG_NAME {
                    consume_lfn_entry(
                        &mut lfn,
                        DirLocation {
                            lba,
                            slot: slot as u8,
                        },
                        &sector[base..base + DIR_ENTRY_SIZE],
                    );
                    free_run_len = 0;
                    continue;
                }
                if (attr & ATTR_VOLUME) != 0 {
                    lfn.clear();
                    free_run_len = 0;
                    continue;
                }

                if let Some(target) = target_name {
                    let record = parse_record(&sector, base, &lfn);
                    if segment_matches_record(target, &record) {
                        let mut lfn_locations = [DirLocation::ZERO; MAX_LFN_SLOTS];
                        let (_, _, lfn_count) = build_display_name(&lfn, &record.short_name);
                        if lfn_count > 0 {
                            lfn_locations[..lfn_count]
                                .copy_from_slice(&lfn.lfn_locations[..lfn_count]);
                        }
                        return Ok(DirLookup {
                            found: Some(DirFound {
                                short_location: DirLocation {
                                    lba,
                                    slot: slot as u8,
                                },
                                lfn_locations,
                                lfn_count: lfn_count as u8,
                                record: DirRecord {
                                    short_name: record.short_name,
                                    display_name: record.display_name,
                                    display_name_len: record.display_name_len,
                                    attr,
                                    first_cluster: record.first_cluster,
                                    size: record.size,
                                },
                            }),
                            free: free_slots,
                        });
                    }
                }
                lfn.clear();
                free_run_len = 0;
            }
        }

        match next_cluster(sd, volume, cluster).await? {
            Some(next) => cluster = next,
            None => {
                return Ok(DirLookup {
                    found: None,
                    free: free_slots,
                });
            }
        }
    }
}

fn classify_directory_slot(first: u8, reached_directory_end: bool) -> (bool, bool) {
    if reached_directory_end {
        return (true, true);
    }
    if first == 0x00 {
        // FAT spec: 0x00 marks this entry free and all following entries free.
        return (true, true);
    }
    if first == 0xE5 {
        return (true, false);
    }
    (false, false)
}

fn record_free_slot(
    free_run: &mut [DirLocation; MAX_LFN_SLOTS + 1],
    free_run_len: &mut usize,
    needed_free_slots: usize,
    free_slots: &mut Option<[DirLocation; MAX_LFN_SLOTS + 1]>,
    location: DirLocation,
) {
    if *free_run_len < free_run.len() {
        free_run[*free_run_len] = location;
        *free_run_len += 1;
    } else {
        free_run.copy_within(1.., 0);
        free_run[free_run.len() - 1] = location;
        *free_run_len = free_run.len();
    }

    if free_slots.is_none() && *free_run_len >= needed_free_slots {
        let start = *free_run_len - needed_free_slots;
        let mut selected = [DirLocation::ZERO; MAX_LFN_SLOTS + 1];
        selected[..needed_free_slots].copy_from_slice(&free_run[start..start + needed_free_slots]);
        *free_slots = Some(selected);
    }
}

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
