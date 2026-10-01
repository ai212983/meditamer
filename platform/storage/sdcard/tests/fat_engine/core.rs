use sdcard::fat::{
    FatEngine, FatEngineError, FatIoAction, FatIoCompletion, FatPayloadId, FatRequest, FatResult,
    FatStep, SdFatError,
};
use sdcard::probe::SdProbeError;

use super::support::{path, run, FakeDisk, DATA_START};

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
        engine.advance(FatIoCompletion::TimedOut(
            sdcard::transport::TransportError::new(
                sdcard::transport::TransportErrorKind::Timeout,
                "test timeout",
                None,
            ),
        )),
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
fn malformed_boot_sector_is_rejected() {
    let mut disk = FakeDisk::fat32();
    disk.sectors.get_mut(&0).unwrap()[510] = 0;
    let mut engine = FatEngine::new();
    let mut output = [0u8; 64];
    let (root, root_len) = path("/");

    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::List {
                path: root,
                path_len: root_len,
            },
            &[],
            &mut output,
        ),
        FatResult::Error(FatEngineError::Fat(SdFatError::InvalidBootSector))
    ));
}

#[test]
fn corrupt_fat_metadata_is_rejected() {
    let mut disk = FakeDisk::fat32();
    // Clearing both FAT-size encodings leaves a signed boot sector with no
    // usable FAT geometry; mounting must reject it before any mutation.
    let boot = disk.sectors.get_mut(&0).unwrap();
    boot[22..24].fill(0);
    boot[36..40].fill(0);
    let mut engine = FatEngine::new();
    let mut output = [0u8; 64];
    let (root, root_len) = path("/");
    assert!(matches!(
        run(
            &mut disk,
            &mut engine,
            FatRequest::List {
                path: root,
                path_len: root_len,
            },
            &[],
            &mut output,
        ),
        FatResult::Error(FatEngineError::Fat(SdFatError::UnsupportedFatType))
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
                        completion = FatIoCompletion::Failed(SdProbeError::HostStub.into());
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
