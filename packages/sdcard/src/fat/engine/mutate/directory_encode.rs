fn utf16_len(name: &[u8]) -> Result<usize, SdFatError> {
    let text = core::str::from_utf8(name).map_err(|_| SdFatError::InvalidLongName)?;
    let mut len = 0usize;
    for ch in text.chars() {
        len = len.saturating_add(ch.len_utf16());
        if len > MAX_LFN_SLOTS * 13 {
            return Err(SdFatError::NameTooLong);
        }
    }
    if len == 0 {
        return Err(SdFatError::InvalidPath);
    }
    Ok(len)
}

fn utf16_unit(name: &[u8], wanted: usize) -> Result<u16, SdFatError> {
    let text = core::str::from_utf8(name).map_err(|_| SdFatError::InvalidLongName)?;
    let mut index = 0usize;
    for ch in text.chars() {
        let mut encoded = [0u16; 2];
        for unit in ch.encode_utf16(&mut encoded).iter().copied() {
            if index == wanted {
                return Ok(unit);
            }
            index += 1;
        }
    }
    Err(SdFatError::InvalidLongName)
}

fn write_lfn_to_sector(
    sector: &mut [u8; SD_SECTOR_SIZE],
    location: DirLocation,
    record: DirRecord,
    entry_index: u8,
    lfn_count: u8,
    lfn_len: usize,
) -> Result<(), SdFatError> {
    let base = location.slot as usize * super::super::DIR_ENTRY_SIZE;
    let entry = &mut sector[base..base + super::super::DIR_ENTRY_SIZE];
    entry.fill(0xFF);
    let sequence = lfn_count.saturating_sub(entry_index);
    entry[0] = sequence | if entry_index == 0 { 0x40 } else { 0 };
    entry[11] = ATTR_LONG_NAME;
    entry[12] = 0;
    entry[13] = short_name_checksum(&record.short_name);
    entry[26] = 0;
    entry[27] = 0;
    let start = (sequence as usize - 1) * 13;
    const OFFSETS: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
    for (part, offset) in OFFSETS.iter().copied().enumerate() {
        let unit_index = start + part;
        let value = if unit_index < lfn_len {
            utf16_unit(
                &record.display_name[..record.display_name_len as usize],
                unit_index,
            )?
        } else if unit_index == lfn_len {
            0
        } else {
            0xFFFF
        };
        entry[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn write_record_to_sector(
    sector: &mut [u8; SD_SECTOR_SIZE],
    location: DirLocation,
    record: DirRecord,
) {
    let base = location.slot as usize * super::super::DIR_ENTRY_SIZE;
    sector[base..base + super::super::DIR_ENTRY_SIZE].fill(0);
    sector[base..base + 11].copy_from_slice(&record.short_name);
    sector[base + 11] = record.attr;
    sector[base + 20..base + 22]
        .copy_from_slice(&((record.first_cluster >> 16) as u16).to_le_bytes());
    sector[base + 26..base + 28].copy_from_slice(&(record.first_cluster as u16).to_le_bytes());
    sector[base + 28..base + 32].copy_from_slice(&record.size.to_le_bytes());
}

fn write_dot_entries(
    sector: &mut [u8; SD_SECTOR_SIZE],
    cluster: u32,
    parent_cluster: u32,
    root_cluster: u32,
) {
    let dot = DirRecord {
        short_name: [
            b'.', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ',
        ],
        display_name: [0; super::super::FAT_NAME_MAX],
        display_name_len: 1,
        attr: super::super::ATTR_DIRECTORY,
        first_cluster: cluster,
        size: 0,
    };
    write_record_to_sector(sector, DirLocation { lba: 0, slot: 0 }, dot);
    let dotdot = DirRecord {
        short_name: [
            b'.', b'.', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ',
        ],
        display_name: [0; super::super::FAT_NAME_MAX],
        display_name_len: 2,
        attr: super::super::ATTR_DIRECTORY,
        first_cluster: if parent_cluster >= 2 {
            parent_cluster
        } else {
            root_cluster
        },
        size: 0,
    };
    write_record_to_sector(sector, DirLocation { lba: 0, slot: 1 }, dotdot);
    sector[super::super::DIR_ENTRY_SIZE * 2] = 0;
}
