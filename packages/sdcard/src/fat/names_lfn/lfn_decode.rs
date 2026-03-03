fn short_name_checksum(short: &[u8; 11]) -> u8 {
    let mut sum = 0u8;
    for byte in short.iter() {
        sum = ((sum & 1) << 7).wrapping_add(sum >> 1).wrapping_add(*byte);
    }
    sum
}

fn lfn_expected_mask(slots: u8) -> u32 {
    if slots == 0 {
        0
    } else {
        (1u32 << slots) - 1
    }
}

fn build_display_name_into(
    lfn: &LfnState,
    short_name: &[u8; 11],
    out: &mut [u8; FAT_NAME_MAX],
) -> (usize, usize) {
    if let Some((len, lfn_count)) = try_build_lfn_display_name(lfn, short_name, out) {
        return (len, lfn_count);
    }

    let short_len = short_name_to_text(short_name, out);
    (short_len, 0)
}

fn build_display_name(lfn: &LfnState, short_name: &[u8; 11]) -> ([u8; FAT_NAME_MAX], usize, usize) {
    let mut out = [0u8; FAT_NAME_MAX];
    let (name_len, lfn_count) = build_display_name_into(lfn, short_name, &mut out);
    (out, name_len, lfn_count)
}

fn try_build_lfn_display_name(
    lfn: &LfnState,
    short_name: &[u8; 11],
    out: &mut [u8; FAT_NAME_MAX],
) -> Option<(usize, usize)> {
    if !lfn_matches_short_name(lfn, short_name) {
        return None;
    }

    let len = copy_lfn_utf16_to_utf8(lfn, out);
    if len == 0 {
        return None;
    }
    Some((len, lfn.expected_slots as usize))
}

fn lfn_matches_short_name(lfn: &LfnState, short_name: &[u8; 11]) -> bool {
    lfn.expected_slots > 0
        && lfn.expected_slots as usize <= MAX_LFN_SLOTS
        && lfn.seen_mask == lfn_expected_mask(lfn.expected_slots)
        && lfn.checksum == short_name_checksum(short_name)
}

fn copy_lfn_utf16_to_utf8(lfn: &LfnState, out: &mut [u8; FAT_NAME_MAX]) -> usize {
    let mut len = 0usize;
    for slot in 0..lfn.expected_slots as usize {
        if !append_lfn_utf16_part(&lfn.utf16_parts[slot], out, &mut len) {
            break;
        }
    }
    len
}

fn append_lfn_utf16_part(part: &[u16; 13], out: &mut [u8; FAT_NAME_MAX], len: &mut usize) -> bool {
    for code in part.iter() {
        if *code == 0x0000 || *code == 0xFFFF {
            return false;
        }
        if let Some(ch) = char::from_u32(*code as u32) {
            let mut tmp = [0u8; 4];
            let encoded = ch.encode_utf8(&mut tmp).as_bytes();
            if *len + encoded.len() > out.len() {
                return false;
            }
            out[*len..*len + encoded.len()].copy_from_slice(encoded);
            *len += encoded.len();
        }
    }
    true
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
    state.seen_mask |= 1u32 << part_idx;
}
