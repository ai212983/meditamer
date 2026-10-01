use sdcard::fat::{
    FatEngine, FatEngineError, FatIoCompletion, FatPayloadId, FatRequest, FatResult, FatStep,
    SdFatError,
};

use super::support::{path, run, FakeDisk};

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

/// Drives a [`FatRequest::Stream`] to completion, collecting every delivered
/// chunk (`workspace().sector[..stream_chunk_len()]`) as `run()` collects a
/// [`FatRequest::Read`]'s single buffer — except here the caller never needs
/// a buffer as large as the file. Mirrors the driving pattern the factory
/// updater (ADR-0014 Phase 1) uses to hash a bundle from bounded memory.
fn run_stream(
    disk: &mut FakeDisk,
    engine: &mut FatEngine,
    request: FatRequest,
) -> (FatResult, Vec<u8>) {
    engine.start(request).unwrap();
    let mut completion = FatIoCompletion::Pending;
    let mut collected = Vec::new();
    let mut delivered = 0u32;
    for _ in 0..200_000 {
        match engine.advance(completion) {
            FatStep::Io(action) => {
                disk.execute(action, engine, &[], &mut []);
                completion = FatIoCompletion::Done;
            }
            step @ (FatStep::Continue | FatStep::Yield) => {
                let now = engine.stream_bytes_delivered();
                if now > delivered {
                    let chunk_len = engine.stream_chunk_len() as usize;
                    collected.extend_from_slice(&engine.workspace().sector[..chunk_len]);
                    delivered = now;
                }
                completion = FatIoCompletion::Pending;
                let _ = step;
            }
            FatStep::Complete(result) => return (result, collected),
        }
    }
    panic!("stream engine failed to complete");
}

#[test]
fn stream_read_delivers_bounded_chunks_across_a_cluster_boundary() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 1536];
    let (file, file_len) = path("/bundle.bin");
    let first = [0x31u8; 500];
    let second = [0x32u8; 700];
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

    // The whole 1200-byte file is spread across 3 one-sector clusters
    // (sectors_per_cluster == 1 in FakeDisk::fat32), so this necessarily
    // exercises the FatReadReturn::Stream cluster-hop path, not just a
    // single-cluster read.
    let (result, collected) = run_stream(
        &mut disk,
        &mut engine,
        FatRequest::Stream {
            path: file,
            path_len: file_len,
        },
    );
    assert!(matches!(result, FatResult::Streamed { bytes: 1200 }));
    assert_eq!(collected.len(), 1200);
    assert_eq!(&collected[..500], &first[..]);
    assert_eq!(&collected[500..1200], &second[..]);

    // A streamed read must reconstruct byte-for-byte what a whole-buffer
    // Read produces from the same file: one content, two delivery shapes.
    output.fill(0);
    let whole = run(
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
    assert!(matches!(whole, FatResult::Read { bytes: 1200 }));
    assert_eq!(&output[..1200], &collected[..]);
}

#[test]
fn stream_read_of_empty_file_completes_with_zero_bytes() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let mut output = [0u8; 8];
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
    let (result, collected) = run_stream(
        &mut disk,
        &mut engine,
        FatRequest::Stream {
            path: file,
            path_len: file_len,
        },
    );
    assert!(matches!(result, FatResult::Streamed { bytes: 0 }));
    assert!(collected.is_empty());
}

#[test]
fn stream_read_of_missing_path_reports_not_found() {
    let mut disk = FakeDisk::fat32();
    let mut engine = FatEngine::new();
    let (file, file_len) = path("/missing.bin");
    let (result, collected) = run_stream(
        &mut disk,
        &mut engine,
        FatRequest::Stream {
            path: file,
            path_len: file_len,
        },
    );
    assert!(matches!(
        result,
        FatResult::Error(FatEngineError::Fat(SdFatError::NotFound))
    ));
    assert!(collected.is_empty());
}
