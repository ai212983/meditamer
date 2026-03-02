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
                match classify_short_name_slot(&sector, base, short_name) {
                    ShortNameSlot::Continue => {}
                    ShortNameSlot::EndOfDirectory => return Ok(false),
                    ShortNameSlot::Match => return Ok(true),
                }
            }
        }
        match next_cluster(sd, volume, cluster).await? {
            Some(next) => cluster = next,
            None => return Ok(false),
        }
    }
}

enum ShortNameSlot {
    Continue,
    EndOfDirectory,
    Match,
}

fn classify_short_name_slot(
    sector: &[u8; SD_SECTOR_SIZE],
    base: usize,
    short_name: &[u8; 11],
) -> ShortNameSlot {
    let first = sector[base];
    if first == 0x00 {
        return ShortNameSlot::EndOfDirectory;
    }
    if first == 0xE5 {
        return ShortNameSlot::Continue;
    }

    let attr = sector[base + 11];
    if attr == ATTR_LONG_NAME || (attr & ATTR_VOLUME) != 0 {
        return ShortNameSlot::Continue;
    }

    let mut existing = [0u8; 11];
    existing.copy_from_slice(&sector[base..base + 11]);
    if &existing == short_name {
        return ShortNameSlot::Match;
    }
    ShortNameSlot::Continue
}

fn make_short_alias(name: &[u8], attempt: u32) -> [u8; 11] {
    let mut out = [b' '; 11];
    let (base, ext) = split_name_parts(name);
    write_alias_extension(&mut out, ext);

    let (digits_buf, digits_len) = suffix_digits(attempt.max(1));
    let max_base = 8usize.saturating_sub(1 + digits_len);
    let mut base_len = write_alias_base(&mut out, base, max_base);
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

fn split_name_parts(name: &[u8]) -> (&[u8], &[u8]) {
    let mut dot = None;
    for (i, byte) in name.iter().enumerate() {
        if *byte == b'.' {
            dot = Some(i);
        }
    }
    match dot {
        Some(idx) => (&name[..idx], &name[idx + 1..]),
        None => (name, &[][..]),
    }
}

fn write_alias_extension(out: &mut [u8; 11], ext: &[u8]) {
    for (ext_len, byte) in ext.iter().enumerate() {
        if ext_len >= 3 {
            break;
        }
        out[8 + ext_len] = normalize_short_char(*byte).unwrap_or(b'_');
    }
}

fn suffix_digits(suffix: u32) -> ([u8; 10], usize) {
    let mut digits_buf = [0u8; 10];
    let mut digits_len = 0usize;
    let mut n = suffix;
    while n > 0 {
        digits_buf[digits_len] = b'0' + (n % 10) as u8;
        digits_len += 1;
        n /= 10;
    }
    (digits_buf, digits_len)
}

fn write_alias_base(out: &mut [u8; 11], base: &[u8], max_base: usize) -> usize {
    let mut base_len = 0usize;
    for byte in base.iter() {
        if base_len >= max_base {
            break;
        }
        out[base_len] = normalize_short_char(*byte).unwrap_or(b'_');
        base_len += 1;
    }
    base_len
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
