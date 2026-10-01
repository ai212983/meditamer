extern crate std;

use super::service::{
    execute_action, run_fat_request, run_fat_request_until, FatBuffers, FatObserver,
};
use super::transport::{SectorTransport, TransportError, TransportErrorKind};
use crate::fat::{
    FatBufferId, FatEngine, FatIoAction, FatIoCompletion, FatPayloadId, FatRequest, FatResult,
};
use embassy_time::Instant;
use std::collections::BTreeMap;

struct Fake {
    reads: usize,
    writes: usize,
    batches: usize,
    recoveries: usize,
    read_error: Option<TransportError>,
    sectors: BTreeMap<u32, [u8; 512]>,
}

struct CancelAfterIo {
    cancel: bool,
    completed: bool,
}
impl CancelAfterIo {
    fn new() -> Self {
        Self {
            cancel: false,
            completed: false,
        }
    }
}
impl FatObserver for CancelAfterIo {
    fn on_stage(&mut self, _stage: crate::fat::FatStageLabel, before_io: bool) {
        if !before_io {
            self.cancel = true;
        }
    }
    fn should_cancel(&self) -> bool {
        self.cancel
    }
    fn on_complete(&mut self, _result: &FatResult) {
        self.completed = true;
    }
}
impl Fake {
    fn new() -> Self {
        Self {
            reads: 0,
            writes: 0,
            batches: 0,
            recoveries: 0,
            read_error: None,
            sectors: BTreeMap::new(),
        }
    }
}
impl SectorTransport for Fake {
    type Error = TransportError;
    async fn read_sector(&mut self, lba: u32, sector: &mut [u8; 512]) -> Result<(), Self::Error> {
        self.reads += 1;
        if let Some(e) = self.read_error {
            return Err(e);
        }
        *sector = self.sectors.get(&lba).copied().unwrap_or([0; 512]);
        Ok(())
    }
    async fn write_sector(&mut self, _: u32, _: &[u8; 512]) -> Result<(), Self::Error> {
        self.writes += 1;
        Ok(())
    }
    async fn write_sectors_contiguous(&mut self, _: u32, _: &[u8]) -> Result<(), Self::Error> {
        self.batches += 1;
        Ok(())
    }
    async fn recover_after_timeout(&mut self) {
        self.recoveries += 1;
    }
}

struct StreamObserver {
    bytes: std::vec::Vec<u8>,
    completed: bool,
}

#[derive(Default)]
struct ErrorObserver {
    error: Option<TransportError>,
}

impl FatObserver for ErrorObserver {
    fn on_transport_error(
        &mut self,
        _stage: crate::fat::FatStageLabel,
        _action: FatIoAction,
        error: TransportError,
    ) {
        self.error = Some(error);
    }
}
impl StreamObserver {
    fn new() -> Self {
        Self {
            bytes: std::vec::Vec::new(),
            completed: false,
        }
    }
}
impl FatObserver for StreamObserver {
    fn on_stream_chunk(&mut self, _offset: u32, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }
    fn on_complete(&mut self, _result: &FatResult) {
        self.completed = true;
    }
}

fn block_on<F: core::future::Future>(future: F) -> F::Output {
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    unsafe fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &VTABLE)
    }
    unsafe fn wake(_: *const ()) {}
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake, wake);
    let waker = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut future = core::pin::pin!(future);
    loop {
        if let Poll::Ready(v) = future.as_mut().poll(&mut cx) {
            return v;
        }
    }
}
fn payloads<'a>(input: &'a [u8], output: &'a mut [u8]) -> FatBuffers<'a> {
    FatBuffers { input, output }
}

#[test]
fn bounds_are_rejected_without_io() {
    let mut t = Fake::new();
    let mut e = FatEngine::new();
    let mut out = [0; 512];
    let mut b = payloads(&[], &mut out);
    let c = block_on(execute_action(
        FatIoAction::ReadSectorToPayload {
            lba: 1,
            buffer: FatBufferId::Sector,
            payload: FatPayloadId::Primary,
            payload_offset: 0,
            sector_offset: 0,
            len: 513,
        },
        &mut t,
        &mut e,
        &mut b,
    ));
    assert!(matches!(c, FatIoCompletion::InvalidState));
    assert_eq!(t.reads, 0);
}

#[test]
fn failed_read_leaves_output_unchanged() {
    let mut t = Fake::new();
    t.read_error = Some(TransportError::new(
        TransportErrorKind::Integrity,
        "crc",
        Some(7),
    ));
    let mut e = FatEngine::new();
    let mut out = [0xA5u8; 8];
    let before = out;
    let mut b = payloads(&[], &mut out);
    let c = block_on(execute_action(
        FatIoAction::ReadSectorToPayload {
            lba: 1,
            buffer: FatBufferId::Sector,
            payload: FatPayloadId::Primary,
            payload_offset: 2,
            sector_offset: 0,
            len: 4,
        },
        &mut t,
        &mut e,
        &mut b,
    ));
    assert!(matches!(c, FatIoCompletion::Failed(_)));
    assert_eq!(*b.output, before);
}

#[test]
fn sector_offset_copies_source_slice() {
    let mut t = Fake::new();
    let mut sector = [0u8; 512];
    for (index, byte) in sector.iter_mut().enumerate() {
        *byte = index as u8;
    }
    t.sectors.insert(7, sector);
    let mut e = FatEngine::new();
    let mut out = [0u8; 32];
    let mut b = payloads(&[], &mut out);
    let c = block_on(execute_action(
        FatIoAction::ReadSectorToPayload {
            lba: 7,
            buffer: FatBufferId::Sector,
            payload: FatPayloadId::Primary,
            payload_offset: 4,
            sector_offset: 100,
            len: 16,
        },
        &mut t,
        &mut e,
        &mut b,
    ));
    assert!(matches!(c, FatIoCompletion::Done));
    assert_eq!(t.reads, 1);
    assert_eq!(&b.output[4..20], &sector[100..116]);
    assert!(b.output[..4].iter().all(|v| *v == 0));
    assert!(b.output[20..].iter().all(|v| *v == 0));
}

#[test]
fn source_bounds_are_rejected_before_io() {
    let mut t = Fake::new();
    let mut e = FatEngine::new();
    let mut out = [0u8; 512];
    let mut b = payloads(&[], &mut out);
    let c = block_on(execute_action(
        FatIoAction::ReadSectorToPayload {
            lba: 1,
            buffer: FatBufferId::Sector,
            payload: FatPayloadId::Primary,
            payload_offset: 0,
            sector_offset: 500,
            len: 13,
        },
        &mut t,
        &mut e,
        &mut b,
    ));
    assert!(matches!(c, FatIoCompletion::InvalidState));
    assert_eq!(t.reads, 0);
}

#[test]
fn contiguous_write_remains_one_batch() {
    let mut t = Fake::new();
    let mut e = FatEngine::new();
    let input = [0x5Au8; 1024];
    let mut out = [];
    let mut b = payloads(&input, &mut out);
    let c = block_on(execute_action(
        FatIoAction::WritePayloadSectors {
            start_lba: 4,
            payload: FatPayloadId::Primary,
            payload_offset: 0,
            sectors: 2,
        },
        &mut t,
        &mut e,
        &mut b,
    ));
    assert!(matches!(c, FatIoCompletion::Done));
    assert_eq!((t.batches, t.writes), (1, 0));
}

#[test]
fn invalid_metadata_does_not_write() {
    let mut t = Fake::new();
    t.read_error = Some(TransportError::new(
        TransportErrorKind::Integrity,
        "metadata crc",
        Some(0x1234),
    ));
    let mut e = FatEngine::new();
    let mut out = [];
    let mut b = payloads(&[], &mut out);
    let path = [b'/'; crate::SD_PATH_MAX];
    let r = block_on(run_fat_request(
        FatRequest::List { path, path_len: 1 },
        &mut t,
        &mut e,
        &mut b,
        &mut (),
    ));
    assert!(matches!(r, FatResult::Error(_)));
    assert_eq!(t.writes, 0);
}

#[test]
fn timeout_recovers_and_invalidates() {
    let mut t = Fake::new();
    let timeout = TransportError::new(TransportErrorKind::Timeout, "data token timeout", Some(17))
        .with_timeout_wait(251, 3);
    t.read_error = Some(timeout);
    let mut e = FatEngine::new();
    let mut out = [];
    let mut b = payloads(&[], &mut out);
    let path = [b'/'; crate::SD_PATH_MAX];
    let mut observer = ErrorObserver::default();
    let r = block_on(run_fat_request(
        FatRequest::List { path, path_len: 1 },
        &mut t,
        &mut e,
        &mut b,
        &mut observer,
    ));
    assert!(matches!(
        r,
        FatResult::Error(crate::fat::FatEngineError::TimedOut)
    ));
    assert_eq!(t.recoveries, 1);
    assert_eq!(observer.error, Some(timeout));
    assert!(!e.is_busy());
}

#[test]
fn stream_observer_receives_each_chunk_in_order() {
    let mut t = Fake::new();
    let mut boot = [0u8; 512];
    boot[11..13].copy_from_slice(&512u16.to_le_bytes());
    boot[13] = 1;
    boot[14..16].copy_from_slice(&1u16.to_le_bytes());
    boot[16] = 1;
    boot[32..36].copy_from_slice(&66_038u32.to_le_bytes());
    boot[36..40].copy_from_slice(&512u32.to_le_bytes());
    boot[44..48].copy_from_slice(&2u32.to_le_bytes());
    boot[510] = 0x55;
    boot[511] = 0xAA;
    t.sectors.insert(0, boot);
    let mut fat = [0u8; 512];
    fat[0..4].copy_from_slice(&0x0FFF_FFF8u32.to_le_bytes());
    fat[4..8].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
    fat[8..12].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
    fat[12..16].copy_from_slice(&4u32.to_le_bytes());
    fat[16..20].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
    t.sectors.insert(1, fat);
    let mut dir = [0u8; 512];
    dir[0..11].copy_from_slice(b"STREAM  BIN");
    dir[11] = 0x20;
    dir[26..28].copy_from_slice(&3u16.to_le_bytes());
    dir[28..32].copy_from_slice(&1024u32.to_le_bytes());
    t.sectors.insert(513, dir);
    let mut first = [0x11u8; 512];
    let mut second = [0x22u8; 512];
    first[0] = 0xA1;
    second[0] = 0xB2;
    t.sectors.insert(514, first);
    t.sectors.insert(515, second);
    let mut e = FatEngine::new();
    let mut out = [];
    let mut b = payloads(&[], &mut out);
    let (path, path_len) = {
        let mut p = [0u8; crate::SD_PATH_MAX];
        p[..11].copy_from_slice(b"/STREAM.BIN");
        (p, 11)
    };
    let mut observer = StreamObserver::new();
    let result = block_on(run_fat_request(
        FatRequest::Stream { path, path_len },
        &mut t,
        &mut e,
        &mut b,
        &mut observer,
    ));
    assert!(
        matches!(result, FatResult::Streamed { bytes: 1024 }),
        "{result:?}"
    );
    assert_eq!(observer.bytes.len(), 1024);
    assert_eq!(observer.bytes[0], 0xA1);
    assert_eq!(observer.bytes[512], 0xB2);
    assert!(observer.completed);
}

#[test]
fn expired_deadline_is_rejected_before_transport_io() {
    let mut t = Fake::new();
    let mut e = FatEngine::new();
    let mut out = [];
    let mut b = payloads(&[], &mut out);
    let path = [b'/'; crate::SD_PATH_MAX];
    let result = block_on(run_fat_request_until(
        FatRequest::List { path, path_len: 1 },
        &mut t,
        &mut e,
        &mut b,
        &mut (),
        Instant::now(),
    ));
    assert!(matches!(
        result,
        FatResult::Error(crate::fat::FatEngineError::TimedOut)
    ));
    assert_eq!(t.reads, 0);
    assert!(!e.is_busy());
}

#[test]
fn cancellation_after_io_stops_follow_on_writes() {
    let mut t = Fake::new();
    let mut e = FatEngine::new();
    let mut out = [];
    let mut b = payloads(&[], &mut out);
    let path = [b'/'; crate::SD_PATH_MAX];
    let mut observer = CancelAfterIo::new();
    let result = block_on(run_fat_request(
        FatRequest::List { path, path_len: 1 },
        &mut t,
        &mut e,
        &mut b,
        &mut observer,
    ));
    assert!(matches!(
        result,
        FatResult::Error(crate::fat::FatEngineError::Cancelled)
    ));
    assert_eq!(t.writes, 0);
    assert!(observer.completed);
    assert!(!e.is_busy());
}
