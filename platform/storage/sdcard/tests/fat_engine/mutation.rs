use sdcard::fat::{FatEngine, FatEngineError, FatPayloadId, FatRequest, FatResult};

use super::support::{path, run, FakeDisk, ROOT_CLUSTER};

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
