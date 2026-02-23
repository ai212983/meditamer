async fn write_dir_slot(
    sd: &mut SdCardProbe<'_>,
    location: &DirLocation,
    entry: &[u8; DIR_ENTRY_SIZE],
) -> Result<(), SdFatError> {
    let mut sector = [0u8; SD_SECTOR_SIZE];
    sd.read_sector(location.lba, &mut sector).await?;
    let base = location.slot as usize * DIR_ENTRY_SIZE;
    sector[base..base + DIR_ENTRY_SIZE].copy_from_slice(entry);
    sd.write_sector(location.lba, &sector).await?;
    Ok(())
}

async fn write_new_entry(
    sd: &mut SdCardProbe<'_>,
    slots: &[DirLocation],
    record: &DirRecord,
    lfn_utf16: &[u16],
) -> Result<(), SdFatError> {
    let lfn_slots = slots.len().saturating_sub(1);
    let checksum = short_name_checksum(&record.short_name);

    for idx in 0..lfn_slots {
        let seq = (lfn_slots - idx) as u8;
        let mut entry = [0xFFu8; DIR_ENTRY_SIZE];
        entry[0] = seq | if idx == 0 { 0x40 } else { 0 };
        entry[11] = ATTR_LONG_NAME;
        entry[12] = 0;
        entry[13] = checksum;
        entry[26] = 0;
        entry[27] = 0;

        let start = (seq as usize - 1) * 13;
        for part_idx in 0..13 {
            let value = if start + part_idx < lfn_utf16.len() {
                lfn_utf16[start + part_idx]
            } else if start + part_idx == lfn_utf16.len() {
                0x0000
            } else {
                0xFFFF
            };
            let bytes = value.to_le_bytes();
            let off = match part_idx {
                0 => 1,
                1 => 3,
                2 => 5,
                3 => 7,
                4 => 9,
                5 => 14,
                6 => 16,
                7 => 18,
                8 => 20,
                9 => 22,
                10 => 24,
                11 => 28,
                _ => 30,
            };
            entry[off] = bytes[0];
            entry[off + 1] = bytes[1];
        }
        write_dir_slot(sd, &slots[idx], &entry).await?;
    }

    write_directory_entry(sd, &slots[lfn_slots], record).await
}

async fn mark_found_deleted(sd: &mut SdCardProbe<'_>, found: &DirFound) -> Result<(), SdFatError> {
    for idx in 0..found.lfn_count as usize {
        mark_slot_deleted(sd, &found.lfn_locations[idx]).await?;
    }
    mark_slot_deleted(sd, &found.short_location).await
}

async fn mark_slot_deleted(sd: &mut SdCardProbe<'_>, location: &DirLocation) -> Result<(), SdFatError> {
    let mut sector = [0u8; SD_SECTOR_SIZE];
    sd.read_sector(location.lba, &mut sector).await?;
    let base = location.slot as usize * DIR_ENTRY_SIZE;
    sector[base] = 0xE5;
    sd.write_sector(location.lba, &sector).await?;
    Ok(())
}

async fn is_directory_empty(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    dir_cluster: u32,
) -> Result<bool, SdFatError> {
    let mut cluster = dir_cluster;
    let mut visited = 0u32;
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
                if first == 0x00 {
                    return Ok(true);
                }
                if first == 0xE5 {
                    continue;
                }
                let attr = sector[base + 11];
                if attr == ATTR_LONG_NAME || (attr & ATTR_VOLUME) != 0 {
                    continue;
                }
                let mut short = [0u8; 11];
                short.copy_from_slice(&sector[base..base + 11]);
                if short == [b'.', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' '] {
                    continue;
                }
                if short == [b'.', b'.', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' '] {
                    continue;
                }
                return Ok(false);
            }
        }
        match next_cluster(sd, volume, cluster).await? {
            Some(next) => cluster = next,
            None => return Ok(true),
        }
    }
}

async fn initialize_directory_cluster(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    dir_cluster: u32,
    parent_cluster: u32,
) -> Result<(), SdFatError> {
    let first_lba = cluster_to_lba(volume, dir_cluster)?;
    for offset in 0..volume.sectors_per_cluster as u32 {
        let zero = [0u8; SD_SECTOR_SIZE];
        sd.write_sector(first_lba + offset, &zero).await?;
    }

    let mut sector = [0u8; SD_SECTOR_SIZE];
    sector[0..11].copy_from_slice(&[b'.', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ']);
    sector[11] = ATTR_DIRECTORY;
    let dot_hi = ((dir_cluster >> 16) as u16).to_le_bytes();
    let dot_lo = (dir_cluster as u16).to_le_bytes();
    sector[20] = dot_hi[0];
    sector[21] = dot_hi[1];
    sector[26] = dot_lo[0];
    sector[27] = dot_lo[1];

    let base = DIR_ENTRY_SIZE;
    sector[base..base + 11]
        .copy_from_slice(&[b'.', b'.', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ']);
    sector[base + 11] = ATTR_DIRECTORY;
    let parent = if parent_cluster >= 2 {
        parent_cluster
    } else {
        volume.root_cluster
    };
    let parent_hi = ((parent >> 16) as u16).to_le_bytes();
    let parent_lo = (parent as u16).to_le_bytes();
    sector[base + 20] = parent_hi[0];
    sector[base + 21] = parent_hi[1];
    sector[base + 26] = parent_lo[0];
    sector[base + 27] = parent_lo[1];

    sector[DIR_ENTRY_SIZE * 2] = 0x00;
    sd.write_sector(first_lba, &sector).await?;
    Ok(())
}

fn encode_short_name(segment: &[u8]) -> Result<[u8; 11], SdFatError> {
    if segment == b"." || segment == b".." {
        let mut out = [b' '; 11];
        out[0] = b'.';
        if segment.len() == 2 {
            out[1] = b'.';
        }
        return Ok(out);
    }

    let mut out = [b' '; 11];
    let dot = segment.iter().position(|&b| b == b'.');
    let (name, ext) = if let Some(dot_idx) = dot {
        let before = &segment[..dot_idx];
        let after = &segment[dot_idx + 1..];
        if after.contains(&b'.') {
            return Err(SdFatError::InvalidShortName);
        }
        (before, after)
    } else {
        (segment, &[][..])
    };

    if name.is_empty() || name.len() > 8 || ext.len() > 3 {
        return Err(SdFatError::InvalidShortName);
    }

    for (i, b) in name.iter().enumerate() {
        out[i] = normalize_short_char(*b)?;
    }
    for (i, b) in ext.iter().enumerate() {
        out[8 + i] = normalize_short_char(*b)?;
    }
    Ok(out)
}

fn normalize_short_char(byte: u8) -> Result<u8, SdFatError> {
    let up = if byte.is_ascii_lowercase() {
        byte.to_ascii_uppercase()
    } else {
        byte
    };
    if up.is_ascii_alphanumeric() || matches!(up, b'_' | b'-' | b'$' | b'~') {
        Ok(up)
    } else {
        Err(SdFatError::InvalidShortName)
    }
}

fn short_name_to_text(raw: &[u8; 11], out: &mut [u8]) -> usize {
    let mut len = 0usize;

    for &b in &raw[0..8] {
        if b == b' ' {
            break;
        }
        if len >= out.len() {
            return len;
        }
        out[len] = b;
        len += 1;
    }

    let has_ext = raw[8..11].iter().any(|&b| b != b' ');
    if has_ext {
        if len >= out.len() {
            return len;
        }
        out[len] = b'.';
        len += 1;
        for &b in &raw[8..11] {
            if b == b' ' {
                break;
            }
            if len >= out.len() {
                return len;
            }
            out[len] = b;
            len += 1;
        }
    }

    len
}

