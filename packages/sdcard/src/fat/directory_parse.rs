use crate::{
    fat::{
        names_lfn::{build_display_name, consume_lfn_entry, parse_record, segment_matches_record},
        DirFound, DirLocation, DirRecord, LfnState, PathSegment, ATTR_LONG_NAME, ATTR_VOLUME,
        DIR_ENTRY_SIZE, MAX_LFN_SLOTS,
    },
    probe::SD_SECTOR_SIZE,
};

#[derive(Clone, Copy)]
pub(super) enum DirectorySlotOutcome {
    Continue,
    Found,
    EarlyReturnFree,
}

pub(super) struct DirectoryScanState {
    pub(super) free_slots: Option<[DirLocation; MAX_LFN_SLOTS + 1]>,
    pub(super) free_run: [DirLocation; MAX_LFN_SLOTS + 1],
    pub(super) free_run_len: usize,
    pub(super) reached_directory_end: bool,
    pub(super) lfn: LfnState,
}

impl DirectoryScanState {
    pub(super) fn new() -> Self {
        Self {
            free_slots: None,
            free_run: [DirLocation::ZERO; MAX_LFN_SLOTS + 1],
            free_run_len: 0,
            reached_directory_end: false,
            lfn: LfnState::new(),
        }
    }
}

pub(super) fn process_directory_slot(
    state: &mut DirectoryScanState,
    sector: &[u8; SD_SECTOR_SIZE],
    lba: u32,
    slot: u8,
    target_name: Option<&PathSegment>,
    needed_free_slots: usize,
    found: &mut Option<DirFound>,
) -> DirectorySlotOutcome {
    let base = slot as usize * DIR_ENTRY_SIZE;
    let first = sector[base];
    let (free, reached_end) = classify_directory_slot(first, state.reached_directory_end);
    state.reached_directory_end = reached_end;

    if free {
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

    *found = target_name.and_then(|target| {
        let record = parse_record(sector, base, &state.lfn);
        if !segment_matches_record(target, &record) {
            return None;
        }
        let mut lfn_locations = [DirLocation::ZERO; MAX_LFN_SLOTS];
        let (_, _, lfn_count) = build_display_name(&state.lfn, &record.short_name);
        if lfn_count > 0 {
            lfn_locations[..lfn_count].copy_from_slice(&state.lfn.lfn_locations[..lfn_count]);
        }
        Some(DirFound {
            short_location: DirLocation { lba, slot },
            lfn_locations,
            lfn_count: lfn_count as u8,
            record: DirRecord { attr, ..record },
        })
    });
    state.lfn.clear();
    state.free_run_len = 0;
    match found {
        Some(_) => DirectorySlotOutcome::Found,
        None => DirectorySlotOutcome::Continue,
    }
}

pub(super) fn classify_directory_slot(first: u8, reached_end: bool) -> (bool, bool) {
    if reached_end || first == 0x00 {
        (true, true)
    } else if first == 0xE5 {
        (true, false)
    } else {
        (false, false)
    }
}

pub(super) fn record_free_slot(
    free_run: &mut [DirLocation; MAX_LFN_SLOTS + 1],
    free_run_len: &mut usize,
    needed: usize,
    selected: &mut Option<[DirLocation; MAX_LFN_SLOTS + 1]>,
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
    if selected.is_none() && *free_run_len >= needed {
        let start = *free_run_len - needed;
        let mut slots = [DirLocation::ZERO; MAX_LFN_SLOTS + 1];
        slots[..needed].copy_from_slice(&free_run[start..start + needed]);
        *selected = Some(slots);
    }
}
