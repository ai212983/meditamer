async fn scan_directory(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    dir_cluster: u32,
    target_name: Option<&PathSegment>,
    needed_free_slots: usize,
) -> Result<DirLookup, SdFatError> {
    let mut cluster = dir_cluster;
    let mut visited = 0u32;
    let mut state = DirectoryScanState::new();

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
                match process_directory_slot(
                    &mut state,
                    &sector,
                    lba,
                    slot as u8,
                    target_name,
                    needed_free_slots,
                ) {
                    DirectorySlotOutcome::Continue => {}
                    DirectorySlotOutcome::EarlyReturnFree => {
                        return Ok(DirLookup {
                            found: None,
                            free: state.free_slots,
                        });
                    }
                    DirectorySlotOutcome::Found(found) => {
                        return Ok(DirLookup {
                            found: Some(found),
                            free: state.free_slots,
                        });
                    }
                }
            }
        }

        match next_cluster(sd, volume, cluster).await? {
            Some(next) => cluster = next,
            None => {
                return Ok(DirLookup {
                    found: None,
                    free: state.free_slots,
                });
            }
        }
    }
}

#[derive(Clone, Copy)]
enum DirectorySlotOutcome {
    Continue,
    Found(DirFound),
    EarlyReturnFree,
}

struct DirectoryScanState {
    free_slots: Option<[DirLocation; MAX_LFN_SLOTS + 1]>,
    free_run: [DirLocation; MAX_LFN_SLOTS + 1],
    free_run_len: usize,
    reached_directory_end: bool,
    lfn: LfnState,
}

impl DirectoryScanState {
    fn new() -> Self {
        Self {
            free_slots: None,
            free_run: [DirLocation::ZERO; MAX_LFN_SLOTS + 1],
            free_run_len: 0,
            reached_directory_end: false,
            lfn: LfnState::new(),
        }
    }
}

fn process_directory_slot(
    state: &mut DirectoryScanState,
    sector: &[u8; SD_SECTOR_SIZE],
    lba: u32,
    slot: u8,
    target_name: Option<&PathSegment>,
    needed_free_slots: usize,
) -> DirectorySlotOutcome {
    let base = slot as usize * DIR_ENTRY_SIZE;
    let first = sector[base];
    let (is_free_slot, next_reached_directory_end) =
        classify_directory_slot(first, state.reached_directory_end);
    state.reached_directory_end = next_reached_directory_end;

    if is_free_slot {
        state.lfn.clear();
        if needed_free_slots > 0 {
            record_free_slot(
                &mut state.free_run,
                &mut state.free_run_len,
                needed_free_slots,
                &mut state.free_slots,
                DirLocation { lba, slot },
            );
            if state.free_slots.is_some() && target_name.is_none() {
                return DirectorySlotOutcome::EarlyReturnFree;
            }
        }
        return DirectorySlotOutcome::Continue;
    }

    let attr = sector[base + 11];
    if attr == ATTR_LONG_NAME {
        consume_lfn_entry(
            &mut state.lfn,
            DirLocation { lba, slot },
            &sector[base..base + DIR_ENTRY_SIZE],
        );
        state.free_run_len = 0;
        return DirectorySlotOutcome::Continue;
    }

    if (attr & ATTR_VOLUME) != 0 {
        state.lfn.clear();
        state.free_run_len = 0;
        return DirectorySlotOutcome::Continue;
    }

    let found = target_name.and_then(|target| {
        find_matching_directory_entry(target, &state.lfn, sector, base, lba, slot, attr)
    });

    state.lfn.clear();
    state.free_run_len = 0;

    if let Some(entry) = found {
        return DirectorySlotOutcome::Found(entry);
    }
    DirectorySlotOutcome::Continue
}

fn find_matching_directory_entry(
    target: &PathSegment,
    lfn: &LfnState,
    sector: &[u8; SD_SECTOR_SIZE],
    base: usize,
    lba: u32,
    slot: u8,
    attr: u8,
) -> Option<DirFound> {
    let record = parse_record(sector, base, lfn);
    if !segment_matches_record(target, &record) {
        return None;
    }
    Some(build_dir_found(lba, slot, attr, lfn, &record))
}

fn build_dir_found(
    lba: u32,
    slot: u8,
    attr: u8,
    lfn: &LfnState,
    record: &DirRecord,
) -> DirFound {
    let mut lfn_locations = [DirLocation::ZERO; MAX_LFN_SLOTS];
    let (_, _, lfn_count) = build_display_name(lfn, &record.short_name);
    if lfn_count > 0 {
        lfn_locations[..lfn_count].copy_from_slice(&lfn.lfn_locations[..lfn_count]);
    }

    DirFound {
        short_location: DirLocation { lba, slot },
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
