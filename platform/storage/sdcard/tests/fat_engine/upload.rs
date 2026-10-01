use sdcard::fat::{
    FatEngine, FatEngineError, FatIoAction, FatPayloadId, FatRequest, FatResult, SdFatError,
};
use sdcard::probe::SD_SECTOR_SIZE;

use super::support::{path, run, FakeDisk, FAT_SECTORS, FAT_START, ROOT_CLUSTER};

#[test]
fn upload_begin_reports_full_volume_and_recovers_after_space_is_freed() {
    let mut disk = FakeDisk::fat32();
    // Mark every FAT entry used, retaining the valid reserved and root-chain
    // markers. This models exhaustion without allocating a real card image.
    for lba in FAT_START..FAT_START + FAT_SECTORS {
        disk.sectors.insert(lba, [0xFF; SD_SECTOR_SIZE]);
    }
    disk.set_fat(0, 0x0FFF_FFF8);
    disk.set_fat(1, 0x0FFF_FFFF);
    disk.set_fat(ROOT_CLUSTER, 0x0FFF_FFFF);

    let mut engine = FatEngine::new();
    let mut output = [0u8; 64];
    let (file, file_len) = path("/full.tmp");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadBegin {
                path: file,
                path_len: file_len,
                expected_size: SD_SECTOR_SIZE as u32,
            },
            &[],
            &mut output,
        ),
        FatResult::Error(FatEngineError::Fat(SdFatError::NoFreeCluster))
    ));

    // A failed allocation completes the request and leaves the engine ready
    // for a new request once the card has space again.
    disk.set_fat(3, 0);
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadBegin {
                path: file,
                path_len: file_len,
                expected_size: SD_SECTOR_SIZE as u32,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
}

#[test]
fn upload_chunks_use_preallocated_session_and_compound_commit() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 1536];
    let first = [0xA5; 1024];
    let second = [0x5A; 512];
    let (file, file_len) = path("/upload.tmp");
    let (destination, destination_len) = path("/final.bin");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Write {
                path: destination,
                path_len: destination_len,
                input: FatPayloadId::Primary,
                input_len: 3,
            },
            b"old",
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadBegin {
                path: file,
                path_len: file_len,
                expected_size: 1536,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(disk.fat(3) >= 0x0FFF_FFF8);
    assert_eq!(disk.fat(4), 5);
    assert_eq!(disk.fat(5), 6);
    assert!(disk.fat(6) >= 0x0FFF_FFF8);
    disk.trace.clear();
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadChunk {
                input: FatPayloadId::Primary,
                input_len: first.len() as u32,
            },
            &first,
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(!disk
        .trace
        .iter()
        .any(|action| matches!(action, FatIoAction::ReadSector { lba: 0, .. })));
    assert!(!disk.trace.iter().any(|action| matches!(
        action,
        FatIoAction::WriteSector { lba, .. }
            if (FAT_START..FAT_START + FAT_SECTORS).contains(lba)
    )));
    assert!(disk
        .trace
        .iter()
        .any(|action| matches!(action, FatIoAction::WritePayloadSectors { sectors: 2, .. })));
    disk.trace.clear();
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadChunk {
                input: FatPayloadId::Primary,
                input_len: second.len() as u32,
            },
            &second,
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(!disk.trace.iter().any(|action| matches!(
        action,
        FatIoAction::WriteSector { lba, .. }
            if (FAT_START..FAT_START + FAT_SECTORS).contains(lba)
    )));
    disk.trace.clear();
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadCommit {
                path: destination,
                path_len: destination_len,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(!disk
        .trace
        .iter()
        .any(|action| matches!(action, FatIoAction::ReadSector { lba: 0, .. })));
    output.fill(0);
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Read {
                path: destination,
                path_len: destination_len,
                output: FatPayloadId::Primary,
                output_capacity: output.len() as u32,
            },
            &[],
            &mut output,
        ),
        FatResult::Read { bytes: 1536 }
    ));
    assert_eq!(&output[..1024], &first);
    assert_eq!(&output[1024..1536], &second);
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadChunk {
                input: FatPayloadId::Primary,
                input_len: 1,
            },
            b"x",
            &mut output,
        ),
        FatResult::Error(FatEngineError::InvalidState)
    ));
}

#[test]
fn aborting_a_large_preallocated_upload_before_commit_does_not_poison_the_temp_path() {
    // Regression test for CAP-0005/CAP-0011: `UploadBegin` links the entire
    // expected-size chain up front but leaves the directory entry's on-disk
    // `size` at 0 until `UploadCommit`. A chain-removal budget derived from
    // that stale `size` (as opposed to the volume's total cluster count) used
    // to be far smaller than the real chain for any upload bigger than a
    // couple dozen clusters, causing both `Remove` and a retried
    // `UploadBegin` on the same never-committed path to fail with a spurious
    // `ClusterChainTooLong` -- indistinguishable from genuine corruption, and
    // permanently poisoning that temp path since nothing could ever remove it.
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 64];
    let (file, file_len) = path("/HCTLUPLD.TMP");
    // 1 sector/cluster * 512 bytes = 512-byte clusters (see FakeDisk::fat32).
    // 50 clusters is well past the old `size(0)-derived + 32` budget of 32.
    let expected_size = 50 * SD_SECTOR_SIZE as u32;

    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadBegin {
                path: file,
                path_len: file_len,
                expected_size,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));

    // Abort before any chunk is written or committed, exactly like the
    // coordinator's grace-expiry abort path: the directory entry's `size` is
    // still 0 on disk, but the chain it points at has ~50 real clusters.
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Remove {
                path: file,
                path_len: file_len,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));

    // A fresh upload attempt to the same path must succeed -- the directory
    // entry and its chain are genuinely gone, not merely reported gone.
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadBegin {
                path: file,
                path_len: file_len,
                expected_size: SD_SECTOR_SIZE as u32,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
}

#[test]
fn retrying_upload_begin_over_a_large_unfinished_chain_does_not_poison_the_temp_path() {
    // Same root cause as the test above, but exercising the other call site:
    // `UploadBegin` retargeting a path that already has a large, never-
    // committed (size still 0 on disk) chain from a prior aborted attempt,
    // without an intervening `Remove`. This is `mutation_start`'s
    // overwrite-before-reallocate branch in mod.rs, not
    // `free_remove_chain_or_delete` in rename_remove.rs.
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 64];
    let (file, file_len) = path("/HCTLUPLD.TMP");
    let large_expected_size = 60 * SD_SECTOR_SIZE as u32;

    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadBegin {
                path: file,
                path_len: file_len,
                expected_size: large_expected_size,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));

    // Retry directly, as a client re-attempting an upload after a dropped
    // connection would -- no `Remove` in between, `size` still 0 on disk.
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadBegin {
                path: file,
                path_len: file_len,
                expected_size: SD_SECTOR_SIZE as u32,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
}
