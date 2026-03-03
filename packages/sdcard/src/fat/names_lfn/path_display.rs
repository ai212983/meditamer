fn parse_path(path: &str, out: &mut [PathSegment; MAX_PATH_SEGMENTS]) -> Result<usize, SdFatError> {
    let bytes = path.as_bytes();
    if bytes.is_empty() {
        return Ok(0);
    }

    let mut idx = 0usize;
    skip_path_separators(bytes, &mut idx);
    if idx == bytes.len() {
        return Ok(0);
    }

    let mut count = 0usize;
    while idx < bytes.len() {
        if count >= MAX_PATH_SEGMENTS {
            return Err(SdFatError::PathTooDeep);
        }

        let segment = next_path_segment(bytes, &mut idx)?;
        out[count] = segment;
        count += 1;

        skip_path_separators(bytes, &mut idx);
    }

    Ok(count)
}

fn skip_path_separators(bytes: &[u8], idx: &mut usize) {
    while *idx < bytes.len() && bytes[*idx] == b'/' {
        *idx += 1;
    }
}

fn next_path_segment(bytes: &[u8], idx: &mut usize) -> Result<PathSegment, SdFatError> {
    let start = *idx;
    while *idx < bytes.len() && bytes[*idx] != b'/' {
        *idx += 1;
    }
    let seg = &bytes[start..*idx];
    if seg.is_empty() || seg.len() > FAT_NAME_MAX {
        return Err(SdFatError::InvalidPath);
    }
    let mut name = [0u8; FAT_NAME_MAX];
    name[..seg.len()].copy_from_slice(seg);
    Ok(PathSegment {
        name,
        len: seg.len() as u8,
    })
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
    let mut display_name = [0u8; FAT_NAME_MAX];
    let (display_name_len, _) = build_display_name_into(lfn, &short_name, &mut display_name);
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
