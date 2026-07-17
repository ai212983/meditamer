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
        consume_lfn_entry(
            lfn,
            DirLocation { lba, slot },
            &sector[base..base + DIR_ENTRY_SIZE],
        );
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
