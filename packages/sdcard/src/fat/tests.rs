#[cfg(test)]
mod tests {
    use super::*;

    fn make_lfn_entry(seq: u8, is_last: bool, checksum: u8, chars: &[u16]) -> [u8; DIR_ENTRY_SIZE] {
        let mut entry = [0xFFu8; DIR_ENTRY_SIZE];
        entry[0] = seq | if is_last { 0x40 } else { 0 };
        entry[11] = ATTR_LONG_NAME;
        entry[12] = 0;
        entry[13] = checksum;
        entry[26] = 0;
        entry[27] = 0;
        let offsets = [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        for (idx, off) in offsets.iter().enumerate() {
            let value = if idx < chars.len() {
                chars[idx]
            } else if idx == chars.len() {
                0x0000
            } else {
                0xFFFF
            };
            let b = value.to_le_bytes();
            entry[*off] = b[0];
            entry[*off + 1] = b[1];
        }
        entry
    }

    #[test]
    fn parse_path_accepts_long_utf8_segments() {
        let mut segments = [PathSegment::EMPTY; MAX_PATH_SEGMENTS];
        let count = parse_path("/Long Folder/ßeta.txt", &mut segments).unwrap();
        assert_eq!(count, 2);
        assert_eq!(segments[0].as_bytes(), b"Long Folder");
        assert_eq!(segments[1].as_bytes(), "ßeta.txt".as_bytes());
    }

    #[test]
    fn build_display_name_from_lfn_sequence() {
        let short = [
            b'L', b'O', b'N', b'G', b'N', b'A', b'~', b'1', b'T', b'X', b'T',
        ];
        let checksum = short_name_checksum(&short);
        let long = "LongFileNameData.txt";
        let mut utf16 = [0u16; 40];
        let mut utf16_len = 0usize;
        for unit in long.encode_utf16() {
            utf16[utf16_len] = unit;
            utf16_len += 1;
        }
        let slots = (utf16_len + 12) / 13;

        let mut lfn = LfnState::new();
        for seq in (1..=slots).rev() {
            let start = (seq - 1) * 13;
            let end = core::cmp::min(start + 13, utf16_len);
            let entry = make_lfn_entry(seq as u8, seq == slots, checksum, &utf16[start..end]);
            consume_lfn_entry(
                &mut lfn,
                DirLocation {
                    lba: 1,
                    slot: seq as u8,
                },
                &entry,
            );
        }

        let (name, len, lfn_count) = build_display_name(&lfn, &short);
        assert_eq!(lfn_count, slots);
        assert_eq!(&name[..len], long.as_bytes());
    }

    #[test]
    fn segment_matches_lfn_case_insensitive_ascii() {
        let mut record = DirRecord {
            short_name: [
                b'L', b'O', b'N', b'G', b'N', b'A', b'~', b'1', b'T', b'X', b'T',
            ],
            display_name: [0; FAT_NAME_MAX],
            display_name_len: 0,
            attr: 0x20,
            first_cluster: 2,
            size: 0,
        };
        let display = b"LongReport.txt";
        record.display_name[..display.len()].copy_from_slice(display);
        record.display_name_len = display.len() as u8;

        let mut seg_name = [0u8; FAT_NAME_MAX];
        seg_name[..display.len()].copy_from_slice(b"longreport.TXT");
        let segment = PathSegment {
            name: seg_name,
            len: display.len() as u8,
        };
        assert!(segment_matches_record(&segment, &record));
    }

    #[test]
    fn clusters_for_size_rounds_up() {
        assert_eq!(clusters_for_size(0, 1024), 0);
        assert_eq!(clusters_for_size(1, 1024), 1);
        assert_eq!(clusters_for_size(1024, 1024), 1);
        assert_eq!(clusters_for_size(1025, 1024), 2);
    }
}
