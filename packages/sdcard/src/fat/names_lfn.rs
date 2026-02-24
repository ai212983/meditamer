fn parse_path(path: &str, out: &mut [PathSegment; MAX_PATH_SEGMENTS]) -> Result<usize, SdFatError> {
    let bytes = path.as_bytes();
    if bytes.is_empty() {
        return Ok(0);
    }

    let mut idx = 0usize;
    while idx < bytes.len() && bytes[idx] == b'/' {
        idx += 1;
    }
    if idx == bytes.len() {
        return Ok(0);
    }

    let mut count = 0usize;
    while idx < bytes.len() {
        if count >= MAX_PATH_SEGMENTS {
            return Err(SdFatError::PathTooDeep);
        }

        let start = idx;
        while idx < bytes.len() && bytes[idx] != b'/' {
            idx += 1;
        }
        let seg = &bytes[start..idx];
        if seg.is_empty() || seg.len() > FAT_NAME_MAX {
            return Err(SdFatError::InvalidPath);
        }
        let mut name = [0u8; FAT_NAME_MAX];
        name[..seg.len()].copy_from_slice(seg);
        out[count] = PathSegment {
            name,
            len: seg.len() as u8,
        };
        count += 1;

        while idx < bytes.len() && bytes[idx] == b'/' {
            idx += 1;
        }
    }

    Ok(count)
}

fn path_segment_to_name(segment: PathSegment) -> [u8; FAT_NAME_MAX] {
    let mut out = [0u8; FAT_NAME_MAX];
    out[..segment.len as usize].copy_from_slice(segment.as_bytes());
    out
}

fn parse_record(sector: &[u8; SD_SECTOR_SIZE], base: usize, lfn: &LfnState) -> DirRecord {
    let mut short_name = [0u8; 11];
    short_name.copy_from_slice(&sector[base..base + 11]);
    let attr = sector[base + 11];
    let cluster_hi = u16::from_le_bytes([sector[base + 20], sector[base + 21]]);
    let cluster_lo = u16::from_le_bytes([sector[base + 26], sector[base + 27]]);
    let first_cluster = ((cluster_hi as u32) << 16) | cluster_lo as u32;
    let size = u32::from_le_bytes([
        sector[base + 28],
        sector[base + 29],
        sector[base + 30],
        sector[base + 31],
    ]);
    let (display_name, display_name_len, _) = build_display_name(lfn, &short_name);
    DirRecord {
        short_name,
        display_name,
        display_name_len: display_name_len as u8,
        attr,
        first_cluster,
        size,
    }
}

fn ascii_eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

fn segment_matches_record(segment: &PathSegment, record: &DirRecord) -> bool {
    let seg = segment.as_bytes();
    if ascii_eq_ignore_case(seg, &record.display_name[..record.display_name_len as usize]) {
        return true;
    }
    let mut short_text = [0u8; 12];
    let short_len = short_name_to_text(&record.short_name, &mut short_text);
    ascii_eq_ignore_case(seg, &short_text[..short_len])
}

fn short_name_checksum(short: &[u8; 11]) -> u8 {
    let mut sum = 0u8;
    for byte in short.iter() {
        sum = ((sum & 1) << 7).wrapping_add(sum >> 1).wrapping_add(*byte);
    }
    sum
}

fn lfn_expected_mask(slots: u8) -> u8 {
    if slots >= 8 {
        0xFF
    } else {
        (1u8 << slots) - 1
    }
}

fn build_display_name(
    lfn: &LfnState,
    short_name: &[u8; 11],
) -> ([u8; FAT_NAME_MAX], usize, usize) {
    let mut out = [0u8; FAT_NAME_MAX];

    if lfn.expected_slots > 0
        && lfn.expected_slots as usize <= MAX_LFN_SLOTS
        && lfn.seen_mask == lfn_expected_mask(lfn.expected_slots)
        && lfn.checksum == short_name_checksum(short_name)
    {
        let mut len = 0usize;
        'outer: for slot in 0..lfn.expected_slots as usize {
            for code in lfn.utf16_parts[slot].iter() {
                if *code == 0x0000 || *code == 0xFFFF {
                    break 'outer;
                }
                if let Some(ch) = char::from_u32(*code as u32) {
                    let mut tmp = [0u8; 4];
                    let encoded = ch.encode_utf8(&mut tmp).as_bytes();
                    if len + encoded.len() > out.len() {
                        break 'outer;
                    }
                    out[len..len + encoded.len()].copy_from_slice(encoded);
                    len += encoded.len();
                }
            }
        }
        if len > 0 {
            return (out, len, lfn.expected_slots as usize);
        }
    }

    let short_len = short_name_to_text(short_name, &mut out);
    (out, short_len, 0)
}

fn consume_lfn_entry(state: &mut LfnState, location: DirLocation, entry: &[u8]) {
    if entry.len() < DIR_ENTRY_SIZE {
        state.clear();
        return;
    }
    let order = entry[0];
    let seq = order & 0x1F;
    if seq == 0 || seq as usize > MAX_LFN_SLOTS {
        state.clear();
        return;
    }

    let checksum = entry[13];
    if (order & 0x40) != 0 {
        state.clear();
        state.expected_slots = seq;
        state.checksum = checksum;
    }
    if state.expected_slots == 0 || seq > state.expected_slots || checksum != state.checksum {
        state.clear();
        return;
    }

    let mut units = [0xFFFFu16; 13];
    let mut idx = 0usize;
    for offset in [1usize, 3, 5, 7, 9] {
        units[idx] = u16::from_le_bytes([entry[offset], entry[offset + 1]]);
        idx += 1;
    }
    for offset in [14usize, 16, 18, 20, 22, 24] {
        units[idx] = u16::from_le_bytes([entry[offset], entry[offset + 1]]);
        idx += 1;
    }
    for offset in [28usize, 30] {
        units[idx] = u16::from_le_bytes([entry[offset], entry[offset + 1]]);
        idx += 1;
    }

    let part_idx = (seq - 1) as usize;
    state.utf16_parts[part_idx] = units;
    state.lfn_locations[part_idx] = location;
    state.seen_mask |= 1 << part_idx;
}

async fn short_name_exists(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    dir_cluster: u32,
    short_name: &[u8; 11],
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
                    return Ok(false);
                }
                if first == 0xE5 {
                    continue;
                }
                let attr = sector[base + 11];
                if attr == ATTR_LONG_NAME || (attr & ATTR_VOLUME) != 0 {
                    continue;
                }
                let mut existing = [0u8; 11];
                existing.copy_from_slice(&sector[base..base + 11]);
                if &existing == short_name {
                    return Ok(true);
                }
            }
        }
        match next_cluster(sd, volume, cluster).await? {
            Some(next) => cluster = next,
            None => return Ok(false),
        }
    }
}

fn make_short_alias(name: &[u8], attempt: u32) -> [u8; 11] {
    let mut out = [b' '; 11];
    let mut dot = None;
    for (i, byte) in name.iter().enumerate() {
        if *byte == b'.' {
            dot = Some(i);
        }
    }
    let (base, ext) = match dot {
        Some(idx) => (&name[..idx], &name[idx + 1..]),
        None => (name, &[][..]),
    };

    let mut ext_len = 0usize;
    for byte in ext.iter() {
        if ext_len >= 3 {
            break;
        }
        out[8 + ext_len] = normalize_short_char(*byte).unwrap_or(b'_');
        ext_len += 1;
    }

    let suffix = attempt.max(1);
    let mut digits_buf = [0u8; 10];
    let mut digits_len = 0usize;
    let mut n = suffix;
    while n > 0 {
        digits_buf[digits_len] = b'0' + (n % 10) as u8;
        digits_len += 1;
        n /= 10;
    }
    let max_base = 8usize.saturating_sub(1 + digits_len);
    let mut base_len = 0usize;
    for byte in base.iter() {
        if base_len >= max_base {
            break;
        }
        out[base_len] = normalize_short_char(*byte).unwrap_or(b'_');
        base_len += 1;
    }
    if base_len == 0 {
        out[0] = b'F';
        out[1] = b'I';
        out[2] = b'L';
        out[3] = b'E';
        base_len = 4.min(max_base);
    }
    if base_len < 8 {
        out[base_len] = b'~';
        base_len += 1;
        for idx in 0..digits_len {
            if base_len >= 8 {
                break;
            }
            out[base_len] = digits_buf[digits_len - 1 - idx];
            base_len += 1;
        }
    }

    out
}

async fn select_new_entry_name(
    sd: &mut SdCardProbe<'_>,
    volume: &Fat32Volume,
    parent_cluster: u32,
    desired: &[u8],
) -> Result<([u8; 11], [u16; MAX_LFN_SLOTS * 13], usize), SdFatError> {
    if let Ok(short) = encode_short_name(desired) {
        if !short_name_exists(sd, volume, parent_cluster, &short).await? {
            return Ok((short, [0u16; MAX_LFN_SLOTS * 13], 0));
        }
    }

    let text = core::str::from_utf8(desired).map_err(|_| SdFatError::InvalidLongName)?;
    let mut utf16 = [0u16; MAX_LFN_SLOTS * 13];
    let mut utf16_len = 0usize;
    for ch in text.chars() {
        let mut tmp = [0u16; 2];
        for unit in ch.encode_utf16(&mut tmp).iter().copied() {
            if utf16_len >= utf16.len() {
                return Err(SdFatError::NameTooLong);
            }
            utf16[utf16_len] = unit;
            utf16_len += 1;
        }
    }
    if utf16_len == 0 {
        return Err(SdFatError::InvalidPath);
    }

    for attempt in 1..10_000 {
        let short = make_short_alias(desired, attempt);
        if !short_name_exists(sd, volume, parent_cluster, &short).await? {
            return Ok((short, utf16, utf16_len));
        }
    }

    Err(SdFatError::DirFull)
}

