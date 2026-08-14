#![cfg(feature = "host-tests")]

use std::collections::BTreeMap;

use sdcard::fat::{
    FatEngine, FatEngineError, FatIoAction, FatIoCompletion, FatPayloadId, FatRequest, FatResult,
    FatStep, SdFatError,
};
use sdcard::probe::{SdProbeError, SD_SECTOR_SIZE};

const FAT_START: u32 = 1;
const FAT_SECTORS: u32 = 512;
const DATA_START: u32 = FAT_START + FAT_SECTORS;
const ROOT_CLUSTER: u32 = 2;

struct FakeDisk {
    sectors: BTreeMap<u32, [u8; SD_SECTOR_SIZE]>,
    trace: Vec<FatIoAction>,
}

impl FakeDisk {
    fn fat32() -> Self {
        let mut disk = Self {
            sectors: BTreeMap::new(),
            trace: Vec::new(),
        };
        let mut boot = [0u8; SD_SECTOR_SIZE];
        boot[11..13].copy_from_slice(&(SD_SECTOR_SIZE as u16).to_le_bytes());
        boot[13] = 1;
        boot[14..16].copy_from_slice(&1u16.to_le_bytes());
        boot[16] = 1;
        boot[32..36].copy_from_slice(&66_038u32.to_le_bytes());
        boot[36..40].copy_from_slice(&FAT_SECTORS.to_le_bytes());
        boot[44..48].copy_from_slice(&ROOT_CLUSTER.to_le_bytes());
        boot[510] = 0x55;
        boot[511] = 0xAA;
        disk.sectors.insert(0, boot);
        disk.set_fat(0, 0x0FFF_FFF8);
        disk.set_fat(1, 0x0FFF_FFFF);
        disk.set_fat(ROOT_CLUSTER, 0x0FFF_FFFF);
        disk
    }

    fn set_fat(&mut self, cluster: u32, value: u32) {
        let offset = cluster as usize * 4;
        let lba = FAT_START + (offset / SD_SECTOR_SIZE) as u32;
        let index = offset % SD_SECTOR_SIZE;
        self.sectors.entry(lba).or_insert([0; SD_SECTOR_SIZE])[index..index + 4]
            .copy_from_slice(&value.to_le_bytes());
    }

    fn fat(&self, cluster: u32) -> u32 {
        let offset = cluster as usize * 4;
        let lba = FAT_START + (offset / SD_SECTOR_SIZE) as u32;
        let index = offset % SD_SECTOR_SIZE;
        u32::from_le_bytes(self.read(lba)[index..index + 4].try_into().unwrap()) & 0x0FFF_FFFF
    }

    fn read(&self, lba: u32) -> [u8; SD_SECTOR_SIZE] {
        self.sectors
            .get(&lba)
            .copied()
            .unwrap_or([0; SD_SECTOR_SIZE])
    }

    fn execute(
        &mut self,
        action: FatIoAction,
        engine: &mut FatEngine,
        input: &[u8],
        output: &mut [u8],
    ) {
        self.trace.push(action);
        match action {
            FatIoAction::ReadSector { lba, .. } => {
                engine.workspace_mut().sector = self.read(lba);
            }
            FatIoAction::WriteSector { lba, .. } => {
                self.sectors.insert(lba, engine.workspace().sector);
            }
            FatIoAction::ReadSectorToPayload {
                lba,
                payload_offset,
                len,
                ..
            } => {
                engine.workspace_mut().sector = self.read(lba);
                let start = payload_offset as usize;
                output[start..start + len as usize]
                    .copy_from_slice(&engine.workspace().sector[..len as usize]);
            }
            FatIoAction::WriteSectorFromPayload {
                lba,
                payload_offset,
                sector_offset,
                len,
                preserve_existing,
                ..
            } => {
                if !preserve_existing {
                    engine.workspace_mut().sector.fill(0);
                }
                let src = payload_offset as usize;
                let dst = sector_offset as usize;
                engine.workspace_mut().sector[dst..dst + len as usize]
                    .copy_from_slice(&input[src..src + len as usize]);
                self.sectors.insert(lba, engine.workspace().sector);
            }
            FatIoAction::WritePayloadSectors {
                start_lba,
                payload_offset,
                sectors,
                ..
            } => {
                for sector in 0..u32::from(sectors) {
                    let start = payload_offset as usize + sector as usize * SD_SECTOR_SIZE;
                    let mut data = [0u8; SD_SECTOR_SIZE];
                    data.copy_from_slice(&input[start..start + SD_SECTOR_SIZE]);
                    self.sectors.insert(start_lba + sector, data);
                }
            }
        }
    }
}

fn path(value: &str) -> ([u8; sdcard::SD_PATH_MAX], u8) {
    let mut out = [0u8; sdcard::SD_PATH_MAX];
    out[..value.len()].copy_from_slice(value.as_bytes());
    (out, value.len() as u8)
}

fn run(
    disk: &mut FakeDisk,
    engine: &mut FatEngine,
    request: FatRequest,
    input: &[u8],
    output: &mut [u8],
) -> FatResult {
    engine.start(request).unwrap();
    let mut completion = FatIoCompletion::Pending;
    for _ in 0..200_000 {
        match engine.advance(completion) {
            FatStep::Io(action) => {
                assert!(engine.has_outstanding_io());
                disk.execute(action, engine, input, output);
                completion = FatIoCompletion::Done;
            }
            FatStep::Continue | FatStep::Yield => completion = FatIoCompletion::Pending,
            FatStep::Complete(result) => return result,
        }
    }
    panic!("engine failed to complete");
}

#[test]
fn short_name_lifecycle_and_traces() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 64];
    let (dir, dir_len) = path("/test");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Mkdir {
                path: dir,
                path_len: dir_len
            },
            &[],
            &mut output
        ),
        FatResult::Done
    ));
    let (child, child_len) = path("/test/child.txt");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Write {
                path: child,
                path_len: child_len,
                input: FatPayloadId::Primary,
                input_len: 1,
            },
            b"x",
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Remove {
                path: dir,
                path_len: dir_len,
            },
            &[],
            &mut output,
        ),
        FatResult::Error(FatEngineError::Fat(SdFatError::NotEmpty))
    ));
    let (file, file_len) = path("/test/a.txt");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Write {
                path: file,
                path_len: file_len,
                input: FatPayloadId::Primary,
                input_len: 5
            },
            b"hello",
            &mut output
        ),
        FatResult::Done
    ));
    output.fill(0);
    let result = run(
        &mut disk,
        &mut engine,
        FatRequest::Read {
            path: file,
            path_len: file_len,
            output: FatPayloadId::Primary,
            output_capacity: output.len() as u32,
        },
        &[],
        &mut output,
    );
    assert!(matches!(result, FatResult::Read { bytes: 5 }));
    assert_eq!(&output[..5], b"hello");
    assert!(disk
        .trace
        .iter()
        .any(|action| matches!(action, FatIoAction::WriteSectorFromPayload { len: 5, .. })));
}

#[test]
fn timeout_completes_and_clears_outstanding_action() {
    let mut engine = FatEngine::new();
    let (path, path_len) = path("/");
    engine.start(FatRequest::List { path, path_len }).unwrap();
    assert!(matches!(
        engine.advance(FatIoCompletion::Pending),
        FatStep::Io(_)
    ));
    assert!(matches!(
        engine.advance(FatIoCompletion::TimedOut),
        FatStep::Complete(FatResult::Error(_))
    ));
    assert!(!engine.has_outstanding_io());
}

#[test]
fn engine_state_size_is_bounded() {
    assert!(core::mem::size_of::<FatEngine>() <= 8 * 1024);
    assert_eq!(DATA_START, 513);
}

#[test]
fn append_truncate_rename_and_lfn_roundtrip() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 1024];
    let (source, source_len) = path("/a long filename.txt");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Write {
                path: source,
                path_len: source_len,
                input: FatPayloadId::Primary,
                input_len: 3,
            },
            b"abc",
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Append {
                path: source,
                path_len: source_len,
                input: FatPayloadId::Primary,
                input_len: 3,
            },
            b"def",
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Truncate {
                path: source,
                path_len: source_len,
                size: 700,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
    output.fill(0xAA);
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Read {
                path: source,
                path_len: source_len,
                output: FatPayloadId::Primary,
                output_capacity: output.len() as u32,
            },
            &[],
            &mut output,
        ),
        FatResult::Read { bytes: 700 }
    ));
    assert_eq!(&output[..6], b"abcdef");
    assert!(output[6..700].iter().all(|byte| *byte == 0));
    let (destination, destination_len) = path("/renamed.bin");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Rename {
                src_path: source,
                src_path_len: source_len,
                dst_path: destination,
                dst_path_len: destination_len,
                replace: false,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Truncate {
                path: destination,
                path_len: destination_len,
                size: 5,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
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
        FatResult::Read { bytes: 5 }
    ));
    assert_eq!(&output[..5], b"abcde");
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

#[test]
fn list_stat_remove_and_replacement_rename() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 1024];
    let (dir, dir_len) = path("/dir");
    let (source, source_len) = path("/source.bin");
    let (destination, destination_len) = path("/destination.bin");
    for (request, data) in [
        (
            FatRequest::Write {
                path: source,
                path_len: source_len,
                input: FatPayloadId::Primary,
                input_len: 6,
            },
            &b"source"[..],
        ),
        (
            FatRequest::Write {
                path: destination,
                path_len: destination_len,
                input: FatPayloadId::Primary,
                input_len: 11,
            },
            &b"destination"[..],
        ),
    ] {
        assert!(matches!(
            run(&mut disk, &mut engine, request, data, &mut output),
            FatResult::Done
        ));
    }
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Mkdir {
                path: dir,
                path_len: dir_len,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::List {
                path: path("/").0,
                path_len: 1,
            },
            &[],
            &mut output,
        ),
        FatResult::Listed { count: 3 }
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Stat {
                path: source,
                path_len: source_len,
            },
            &[],
            &mut output,
        ),
        FatResult::Stat(entry) if entry.size == 6 && !entry.is_dir
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Rename {
                src_path: source,
                src_path_len: source_len,
                dst_path: destination,
                dst_path_len: destination_len,
                replace: true,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
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
        FatResult::Read { bytes: 6 }
    ));
    assert_eq!(&output[..6], b"source");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Remove {
                path: destination,
                path_len: destination_len,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
}

#[test]
fn append_crosses_cluster_and_upload_clear_invalidates_session() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 1536];
    let (file, file_len) = path("/cross.bin");
    let first = [0x31; 500];
    let second = [0x32; 700];
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Write {
                path: file,
                path_len: file_len,
                input: FatPayloadId::Primary,
                input_len: first.len() as u32,
            },
            &first,
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Append {
                path: file,
                path_len: file_len,
                input: FatPayloadId::Primary,
                input_len: second.len() as u32,
            },
            &second,
            &mut output,
        ),
        FatResult::Done
    ));
    output.fill(0);
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Read {
                path: file,
                path_len: file_len,
                output: FatPayloadId::Primary,
                output_capacity: output.len() as u32,
            },
            &[],
            &mut output,
        ),
        FatResult::Read { bytes: 1200 }
    ));
    assert_eq!(&output[..500], &first);
    assert_eq!(&output[500..1200], &second);

    let (upload, upload_len) = path("/clear.tmp");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadBegin {
                path: upload,
                path_len: upload_len,
                expected_size: 1,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::UploadClear,
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
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
fn every_write_io_stage_accepts_injected_failure_without_stale_action() {
    let (file, file_len) = path("/failure.bin");
    let request = FatRequest::Write {
        path: file,
        path_len: file_len,
        input: FatPayloadId::Primary,
        input_len: 700,
    };
    let input = [0x5A; 700];
    let mut successful_disk = FakeDisk::fat32();
    let mut successful_engine = FatEngine::new();
    let mut output = [0u8; 1024];
    assert!(matches!(
        run(
            &mut successful_disk,
            &mut successful_engine,
            request,
            &input,
            &mut output,
        ),
        FatResult::Done
    ));
    let action_count = successful_disk.trace.len();
    assert!(action_count > 8);

    for fail_at in 0..action_count {
        let mut disk = FakeDisk::fat32();
        let mut engine = FatEngine::new();
        engine.start(request).unwrap();
        let mut completion = FatIoCompletion::Pending;
        let mut io_index = 0;
        let result = loop {
            match engine.advance(completion) {
                FatStep::Io(action) => {
                    assert!(engine.has_outstanding_io());
                    if io_index == fail_at {
                        completion = FatIoCompletion::Failed(SdProbeError::HostStub);
                    } else {
                        disk.execute(action, &mut engine, &input, &mut output);
                        completion = FatIoCompletion::Done;
                    }
                    io_index += 1;
                }
                FatStep::Continue | FatStep::Yield => completion = FatIoCompletion::Pending,
                FatStep::Complete(result) => break result,
            }
        };
        assert!(matches!(result, FatResult::Error(FatEngineError::Io(_))));
        assert!(!engine.has_outstanding_io());
    }
}

#[test]
fn empty_file_append_and_truncate_boundaries() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 1024];
    let (file, file_len) = path("/empty.bin");

    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Write {
                path: file,
                path_len: file_len,
                input: FatPayloadId::Primary,
                input_len: 0,
            },
            &[],
            &mut output,
        ),
        FatResult::Done
    ));
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Append {
                path: file,
                path_len: file_len,
                input: FatPayloadId::Primary,
                input_len: 3,
            },
            b"abc",
            &mut output,
        ),
        FatResult::Done
    ));
    for size in [3, 700, 700, 0] {
        assert!(matches!(
            run(
                &mut disk,
                &mut engine,
                FatRequest::Truncate {
                    path: file,
                    path_len: file_len,
                    size,
                },
                &[],
                &mut output,
            ),
            FatResult::Done
        ));
    }
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Stat {
                path: file,
                path_len: file_len,
            },
            &[],
            &mut output,
        ),
        FatResult::Stat(entry) if entry.size == 0
    ));
}

#[test]
fn directory_extension_and_lfn_alias_collisions_roundtrip() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 2048];
    let mut created = Vec::new();

    for index in 0..12 {
        let name = format!("/collision filename number {index:02}.txt");
        let (file, file_len) = path(&name);
        assert!(matches!(
            run(
                &mut disk,
                &mut engine,
                FatRequest::Write {
                    path: file,
                    path_len: file_len,
                    input: FatPayloadId::Primary,
                    input_len: 1,
                },
                &[index as u8],
                &mut output,
            ),
            FatResult::Done
        ));
        created.push((file, file_len, index as u8));
    }

    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::List {
                path: path("/").0,
                path_len: 1,
            },
            &[],
            &mut output,
        ),
        FatResult::Listed { count: 12 }
    ));
    for (file, file_len, expected) in created {
        output.fill(0);
        assert!(matches!(
            run(
                &mut disk,
                &mut engine,
                FatRequest::Read {
                    path: file,
                    path_len: file_len,
                    output: FatPayloadId::Primary,
                    output_capacity: output.len() as u32,
                },
                &[],
                &mut output,
            ),
            FatResult::Read { bytes: 1 }
        ));
        assert_eq!(output[0], expected);
    }
    assert_ne!(disk.fat(ROOT_CLUSTER), 0x0FFF_FFFF);
}

#[test]
fn fragmented_cluster_chain_writes_reads_and_frees() {
    let mut disk = FakeDisk::fat32();
    disk.set_fat(3, 0x0FFF_FFFF);
    disk.set_fat(5, 0x0FFF_FFFF);
    let mut engine = FatEngine::new();
    let mut output = [0u8; 1024];
    let input = [0xA5; 700];
    let (file, file_len) = path("/fragment.bin");

    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Write {
                path: file,
                path_len: file_len,
                input: FatPayloadId::Primary,
                input_len: input.len() as u32,
            },
            &input,
            &mut output,
        ),
        FatResult::Done
    ));
    assert_eq!(disk.fat(4), 6);
    assert!(disk.fat(6) >= 0x0FFF_FFF8);
    output.fill(0);
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::Read {
                path: file,
                path_len: file_len,
                output: FatPayloadId::Primary,
                output_capacity: output.len() as u32,
            },
            &[],
            &mut output,
        ),
        FatResult::Read { bytes: 700 }
    ));
    assert_eq!(&output[..700], &input);
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
    assert_eq!(disk.fat(4), 0);
    assert_eq!(disk.fat(6), 0);
}
