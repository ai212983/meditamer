use sdcard::fat::{
    FatEngine, FatEngineError, FatIoAction, FatPayloadId, FatRequest, FatResult, SdFatError,
};

use super::support::{path, run, FakeDisk, DATA_START, FAT_START};

fn write_file(
    disk: &mut FakeDisk,
    engine: &mut FatEngine,
    name: &str,
    data: &[u8],
    scratch: &mut [u8],
) {
    let (file, file_len) = path(name);
    let result = run(
        disk,
        engine,
        FatRequest::Write {
            path: file,
            path_len: file_len,
            input: FatPayloadId::Primary,
            input_len: data.len() as u32,
        },
        data,
        scratch,
    );
    assert!(matches!(result, FatResult::Done), "{result:?}");
}

fn append_file(
    disk: &mut FakeDisk,
    engine: &mut FatEngine,
    name: &str,
    data: &[u8],
    scratch: &mut [u8],
) {
    let (file, file_len) = path(name);
    let result = run(
        disk,
        engine,
        FatRequest::Append {
            path: file,
            path_len: file_len,
            input: FatPayloadId::Primary,
            input_len: data.len() as u32,
        },
        data,
        scratch,
    );
    assert!(matches!(result, FatResult::Done), "{result:?}");
}

fn read_range(
    disk: &mut FakeDisk,
    engine: &mut FatEngine,
    name: &str,
    offset: u32,
    len: u32,
    output: &mut [u8],
) -> FatResult {
    let (file, file_len) = path(name);
    run(
        disk,
        engine,
        FatRequest::ReadRange {
            path: file,
            path_len: file_len,
            offset,
            len,
            output: FatPayloadId::Primary,
            output_capacity: output.len() as u32,
        },
        &[],
        output,
    )
}

fn has_data_reads(disk: &FakeDisk) -> bool {
    disk.trace
        .iter()
        .any(|action| matches!(action, FatIoAction::ReadSectorToPayload { .. }))
}

/// 1200-byte fixture: 500 `0x31` bytes followed by 700 `0x32` bytes, spread
/// over three one-sector clusters (`sectors_per_cluster == 1`, so every
/// sector hop is also a cluster hop).
fn write_two_part_file(
    disk: &mut FakeDisk,
    engine: &mut FatEngine,
    scratch: &mut [u8],
) -> std::vec::Vec<u8> {
    let first = [0x31u8; 500];
    let second = [0x32u8; 700];
    write_file(disk, engine, "/range.bin", &first, scratch);
    append_file(disk, engine, "/range.bin", &second, scratch);
    let mut full = std::vec::Vec::with_capacity(1200);
    full.extend_from_slice(&first);
    full.extend_from_slice(&second);
    full
}

#[test]
fn range_within_one_sector_reads_unaligned_slice() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 512];
    let data: std::vec::Vec<u8> = (0..100u32).map(|i| i as u8).collect();
    write_file(&mut disk, &mut engine, "/tiny.bin", &data, &mut scratch);

    let mut output = [0u8; 64];
    let result = read_range(&mut disk, &mut engine, "/tiny.bin", 10, 20, &mut output);
    assert!(
        matches!(result, FatResult::Read { bytes: 20 }),
        "{result:?}"
    );
    assert_eq!(&output[..20], &data[10..30]);
}

#[test]
fn range_crossing_sector_boundary() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 1536];
    let full = write_two_part_file(&mut disk, &mut engine, &mut scratch);

    // 500..600 straddles the sector 0 -> sector 1 boundary at byte 512.
    let mut output = [0u8; 128];
    let result = read_range(&mut disk, &mut engine, "/range.bin", 500, 100, &mut output);
    assert!(
        matches!(result, FatResult::Read { bytes: 100 }),
        "{result:?}"
    );
    assert_eq!(&output[..100], &full[500..600]);
}

#[test]
fn range_crossing_cluster_boundary() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 1536];
    let full = write_two_part_file(&mut disk, &mut engine, &mut scratch);

    // 400..1100 walks two cluster hops (sectors end at 512 and 1024).
    let mut output = [0u8; 768];
    let result = read_range(&mut disk, &mut engine, "/range.bin", 400, 700, &mut output);
    assert!(
        matches!(result, FatResult::Read { bytes: 700 }),
        "{result:?}"
    );
    assert_eq!(&output[..700], &full[400..1100]);
}

#[test]
fn range_crossing_fragmented_chain() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 1536];
    let first: std::vec::Vec<u8> = (0..512u32).map(|i| (i % 251) as u8).collect();
    let second: std::vec::Vec<u8> = (0..512u32).map(|i| (255 - i % 251) as u8).collect();
    write_file(&mut disk, &mut engine, "/frag-a.bin", &first, &mut scratch);
    write_file(
        &mut disk,
        &mut engine,
        "/frag-b.bin",
        &[0xBBu8; 512],
        &mut scratch,
    );
    // The append lands on the next free cluster past frag-b's, so frag-a's
    // chain jumps over it instead of running contiguously.
    append_file(&mut disk, &mut engine, "/frag-a.bin", &second, &mut scratch);
    assert_eq!(disk.fat(3), 5);

    let mut full = std::vec::Vec::with_capacity(1024);
    full.extend_from_slice(&first);
    full.extend_from_slice(&second);
    // 400..700 reads the tail of the first cluster and the head of the
    // non-adjacent second cluster.
    let mut output = [0u8; 320];
    let result = read_range(&mut disk, &mut engine, "/frag-a.bin", 400, 300, &mut output);
    assert!(
        matches!(result, FatResult::Read { bytes: 300 }),
        "{result:?}"
    );
    assert_eq!(&output[..300], &full[400..700]);
}

#[test]
fn range_truncates_at_eof_and_reports_actual_bytes() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 512];
    let data: std::vec::Vec<u8> = (0..100u32).map(|i| (i + 7) as u8).collect();
    write_file(&mut disk, &mut engine, "/tiny.bin", &data, &mut scratch);

    let mut output = [0u8; 64];
    let result = read_range(&mut disk, &mut engine, "/tiny.bin", 80, 50, &mut output);
    assert!(
        matches!(result, FatResult::Read { bytes: 20 }),
        "{result:?}"
    );
    assert_eq!(&output[..20], &data[80..100]);
}

#[test]
fn range_at_eof_returns_zero_without_data_reads() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 512];
    let data = [0x5Au8; 100];
    write_file(&mut disk, &mut engine, "/tiny.bin", &data, &mut scratch);

    for (offset, len) in [(100, 10), (100, 0), (50, 0)] {
        disk.trace.clear();
        let mut output = [0xA5u8; 16];
        let result = read_range(
            &mut disk,
            &mut engine,
            "/tiny.bin",
            offset,
            len,
            &mut output,
        );
        assert!(
            matches!(result, FatResult::Read { bytes: 0 }),
            "offset={offset} len={len}: {result:?}"
        );
        assert!(
            !has_data_reads(&disk),
            "offset={offset} len={len} issued data reads"
        );
    }
}

#[test]
fn range_beyond_eof_reports_offset_and_size() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 512];
    write_file(
        &mut disk,
        &mut engine,
        "/tiny.bin",
        &[0x5Au8; 100],
        &mut scratch,
    );

    for len in [10, 0] {
        let mut output = [0u8; 16];
        let result = read_range(&mut disk, &mut engine, "/tiny.bin", 101, len, &mut output);
        assert!(
            matches!(
                result,
                FatResult::Error(FatEngineError::Fat(SdFatError::RangeBeyondEof {
                    offset: 101,
                    size: 100
                }))
            ),
            "len={len}: {result:?}"
        );
    }
}

#[test]
fn capacity_is_checked_against_actual_range() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 512];
    let data: std::vec::Vec<u8> = (0..100u32).map(|i| (i + 3) as u8).collect();
    write_file(&mut disk, &mut engine, "/tiny.bin", &data, &mut scratch);

    // actual is 10: a 10-byte buffer succeeds even though the file is 100.
    let mut output = [0u8; 10];
    let result = read_range(&mut disk, &mut engine, "/tiny.bin", 90, 50, &mut output);
    assert!(
        matches!(result, FatResult::Read { bytes: 10 }),
        "{result:?}"
    );
    assert_eq!(&output[..10], &data[90..100]);

    // One byte short fails with needed == actual (10), not the file size.
    let mut short = [0u8; 9];
    let result = read_range(&mut disk, &mut engine, "/tiny.bin", 90, 50, &mut short);
    assert!(
        matches!(
            result,
            FatResult::Error(FatEngineError::Fat(SdFatError::BufferTooSmall {
                needed: 10
            }))
        ),
        "{result:?}"
    );
}

#[test]
fn range_rejects_directories() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 64];
    let (dir, dir_len) = path("/subdir");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Mkdir {
                path: dir,
                path_len: dir_len
            },
            &[],
            &mut scratch,
        ),
        FatResult::Done
    ));

    let mut output = [0u8; 16];
    let result = read_range(&mut disk, &mut engine, "/subdir", 0, 10, &mut output);
    assert!(
        matches!(
            result,
            FatResult::Error(FatEngineError::Fat(SdFatError::IsDirectory))
        ),
        "{result:?}"
    );
}

fn write_fragmented_pair(
    disk: &mut FakeDisk,
    engine: &mut FatEngine,
    scratch: &mut [u8],
) -> std::vec::Vec<u8> {
    let first: std::vec::Vec<u8> = (0..512u32).map(|i| (i % 251) as u8).collect();
    let second: std::vec::Vec<u8> = (0..512u32).map(|i| (255 - i % 251) as u8).collect();
    write_file(disk, engine, "/frag-a.bin", &first, scratch);
    write_file(disk, engine, "/frag-b.bin", &[0xBBu8; 512], scratch);
    append_file(disk, engine, "/frag-a.bin", &second, scratch);
    assert_eq!(disk.fat(3), 5);
    let mut full = std::vec::Vec::with_capacity(1024);
    full.extend_from_slice(&first);
    full.extend_from_slice(&second);
    full
}

fn fat_read_present(disk: &FakeDisk) -> bool {
    disk.trace.iter().any(|action| {
        matches!(
            action,
            FatIoAction::ReadSector { lba, .. } if (*lba >= FAT_START && *lba < DATA_START)
        )
    })
}

#[test]
fn range_seek_from_second_cluster_reads_correct_bytes() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 1536];
    let full = write_two_part_file(&mut disk, &mut engine, &mut scratch);

    // offset 512 == one full cluster (sectors_per_cluster == 1), so this
    // takes FatReadReturn::RangeSeek for one FAT hop before reading.
    disk.trace.clear();
    let mut output = [0u8; 320];
    let result = read_range(&mut disk, &mut engine, "/range.bin", 512, 300, &mut output);
    assert!(
        matches!(result, FatResult::Read { bytes: 300 }),
        "{result:?}"
    );
    assert_eq!(&output[..300], &full[512..812]);
    assert!(fat_read_present(&disk), "range seek issued no FAT read");
}

#[test]
fn range_starting_in_fragmented_second_cluster() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 1536];
    let full = write_fragmented_pair(&mut disk, &mut engine, &mut scratch);

    // offset 600 sits in the non-contiguous second cluster (cluster 5, LBA
    // DATA_START + 3); it must seek there, not merely cross into it.
    disk.trace.clear();
    let mut output = [0u8; 256];
    let result = read_range(&mut disk, &mut engine, "/frag-a.bin", 600, 200, &mut output);
    assert!(
        matches!(result, FatResult::Read { bytes: 200 }),
        "{result:?}"
    );
    assert_eq!(&output[..200], &full[600..800]);
    assert!(
        fat_read_present(&disk),
        "fragmented seek issued no FAT read"
    );
    let expected = DATA_START + (5 - 2);
    let contiguous_wrong = DATA_START + (4 - 2);
    assert!(
        disk.trace.iter().any(|action| matches!(
            action,
            FatIoAction::ReadSectorToPayload { lba, .. } if *lba == expected
        )),
        "no data read from fragmented cluster 5 (LBA {expected})"
    );
    assert!(
        !disk.trace.iter().any(|action| matches!(
            action,
            FatIoAction::ReadSectorToPayload { lba, .. } if *lba == contiguous_wrong
        )),
        "data read from contiguous LBA {contiguous_wrong} instead of fragmented chain"
    );
}

#[test]
fn range_spc2_starting_in_second_sector_crosses_sector_boundary() {
    let mut disk = FakeDisk::fat32_with_spc(2);
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 4096];
    let data: std::vec::Vec<u8> = (0..2500u32).map(|i| (i % 251) as u8).collect();
    write_file(&mut disk, &mut engine, "/spc2.bin", &data, &mut scratch);

    // bytes_per_cluster == 1024: offset 600 gives skip_clusters == 0,
    // intra == 600, sector_offset == 1 (second sector), skip == 88.
    // len 500 crosses the sector boundary at byte 1024.
    disk.trace.clear();
    let mut output = [0u8; 600];
    let result = read_range(&mut disk, &mut engine, "/spc2.bin", 600, 500, &mut output);
    assert!(
        matches!(result, FatResult::Read { bytes: 500 }),
        "{result:?}"
    );
    assert_eq!(&output[..500], &data[600..1100]);
    let reads: std::vec::Vec<(u32, u16, u16)> = disk
        .trace
        .iter()
        .filter_map(|action| match *action {
            FatIoAction::ReadSectorToPayload {
                lba,
                sector_offset,
                len,
                ..
            } => Some((lba, sector_offset, len)),
            _ => None,
        })
        .collect();
    assert!(!reads.is_empty(), "no data reads traced");
    // Cluster 3 spans LBAs 515..516; the range starts in its second sector
    // with an 88-byte intra-sector skip, then continues at cluster 4 (517).
    assert_eq!(reads[0].0, DATA_START + 2 + 1, "{reads:?}");
    assert_eq!(reads[0].1, 88, "{reads:?}");
    assert!(
        reads
            .iter()
            .any(|(lba, _, _)| *lba == DATA_START + (4 - 2) * 2),
        "no continuation read from next cluster: {reads:?}"
    );
}

#[test]
fn range_seek_truncated_chain_reports_cluster_chain_too_long() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut scratch = [0u8; 1536];
    write_two_part_file(&mut disk, &mut engine, &mut scratch);

    // File spans clusters 3 -> 4 -> 5; orphan the third link.
    disk.set_fat(4, 0x0FFF_FFFF);
    assert_eq!(disk.fat(3), 4);

    // offset 1050 needs two RangeSeek hops (1050 / 512 == 2); the second
    // hop hits EOC during the seek, not during the later data walk.
    let mut output = [0u8; 64];
    let result = read_range(&mut disk, &mut engine, "/range.bin", 1050, 50, &mut output);
    assert!(
        matches!(
            result,
            FatResult::Error(FatEngineError::Fat(SdFatError::ClusterChainTooLong))
        ),
        "{result:?}"
    );
}
