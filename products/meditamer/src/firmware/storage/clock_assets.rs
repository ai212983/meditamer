#![deny(unsafe_code)]

//! Narrow SD binary-frame loader intake for the analog clock ambient.
//!
//! The UI worker queues at most one outstanding streamed read of
//! `/assets/CLOCK/MAPS.BIN` (the `clock-assets` MAPS.BIN pack: 32-byte header
//! + canonical dial/hand source maps, no pre-rendered minute frames) and polls
//!   for completion later. This keeps the display task free to service shared-I2C
//!   SD power requests while the SD worker owns the transfer: queue, return to
//!   the display loop, service power, poll the result.
//!
//! The nine source maps live in one generic strict-PSRAM runtime bundle as
//! nine contiguous parts. Only small metadata crosses the channels below.
//! Existing `SD_REQUESTS`/`SD_RESULTS` channels, depths, and serial buffers
//! are untouched. On success the UI owns the source bytes while the clock
//! screen is active and releases them after navigation. An inactive UI drops
//! a late result, freeing the buffer. Any error clears the
//! single-outstanding flag on `take`, so retry is always possible.
//!
//! Header validation delegates to the canonical `clock-assets::decode_parts`
//! (exact file size, magic, length field, reserved bytes, IEEE CRC32); this
//! module only maps [`::clock_assets::Error`] to [`AssetReadError`]. The
//! `decode_parts` runs once per loader transfer; render steps borrow the
//! validated regions without recalculating their CRC.
//!
//! See docs/references/memory/meditamer-inkplate/budget.md: no internal-DRAM bulk; the
//! request identity and generation counters stay small.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

#[cfg(not(target_os = "none"))]
use super::super::psram::LargeByteBuffer;
use embassy_sync::channel::TrySendError;
#[cfg(target_os = "none")]
use phase_arena::RuntimeBundle;
use sdcard::request::SdCommand;
use sdcard::SD_PATH_MAX;

// Canonical single owner: `platform/ui/visuals/clock-assets`. Absolute path
// (leading `::`) because this module is itself named `clock_assets`.
pub use ::clock_assets::{crc32, FILE_LEN, HEADER_LEN, MAGIC, PAYLOAD_LEN};

/// SD path of the canonical clock source-map pack. It sits under the existing
/// `/assets` upload subtree, so the current HTTP upload service can stage it
/// with no new transfer mechanism.
pub const ASSET_PATH: &[u8] = b"/assets/CLOCK/MAPS.BIN";

/// Canonical maps held as nine contiguous parts of one generic runtime
/// bundle, so no 2.5 MB contiguous PSRAM allocation is required. The SD
/// owner validates the complete stream before publishing this value.
#[cfg(target_os = "none")]
pub struct AssetBuffers {
    bundle: RuntimeBundle<esp_alloc::ExternalMemory>,
}

#[cfg(target_os = "none")]
impl AssetBuffers {
    pub fn new(bundle: RuntimeBundle<esp_alloc::ExternalMemory>) -> Self {
        Self { bundle }
    }

    pub fn borrowed(&self) -> Option<::clock_assets::ClockAssetParts<'_>> {
        ::clock_assets::ClockAssetParts::borrow_validated([
            self.bundle.part(0)?,
            self.bundle.part(1)?,
            self.bundle.part(2)?,
            self.bundle.part(3)?,
            self.bundle.part(4)?,
            self.bundle.part(5)?,
            self.bundle.part(6)?,
            self.bundle.part(7)?,
            self.bundle.part(8)?,
        ])
        .ok()
    }

    pub fn total_bytes(&self) -> usize {
        self.bundle.total_bytes()
    }
}

#[cfg(not(target_os = "none"))]
pub struct AssetBuffers {
    pub maps: [LargeByteBuffer; 9],
}

#[cfg(not(target_os = "none"))]
impl AssetBuffers {
    pub fn new(maps: [LargeByteBuffer; 9]) -> Self {
        Self { maps }
    }

    pub fn borrowed(&self) -> Option<::clock_assets::ClockAssetParts<'_>> {
        ::clock_assets::ClockAssetParts::borrow_validated(core::array::from_fn(|i| {
            self.maps[i].as_slice()
        }))
        .ok()
    }
}

/// Outcome of one complete validated stream.
pub type AssetReadOutcome = Result<AssetBuffers, AssetReadError>;

/// Errors for the narrow asset-read path. Every variant leaves the loader
/// reusable: the single-outstanding flag is released when the completion is
/// taken (or when the stale completion is replaced), so the UI may retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetReadError {
    /// Another read is already outstanding; poll or drain it first.
    Busy,
    /// The narrow request channel was unexpectedly full.
    QueueFull,
    /// SD power, init, or a transport/engine failure; bounded retry may help.
    Unavailable,
    /// The asset file is absent (or the fixed path is invalid).
    NotFound,
    /// Truncated, over-long, or otherwise wrong-sized file content.
    BadSize,
    /// Header magic mismatch: not a clock source-map pack.
    BadMagic,
    /// Length-field or reserved-byte mismatch in the header.
    BadHeader,
    /// Header CRC32 does not match the payload.
    BadChecksum,
    /// One of the strict-PSRAM map-region allocations failed.
    OutOfMemory,
    /// Allocation fell back to internal RAM; refused to protect the
    /// internal reserve (see docs/references/memory/meditamer-inkplate/budget.md).
    NotPsram,
}

impl AssetReadError {
    /// Whether a bounded retry is worth attempting. Format errors are also
    /// retryable in the sense that the loader is never wedged; they will
    /// simply fail again until the SD source is replaced.
    pub const fn is_retryable(self) -> bool {
        match self {
            Self::Busy | Self::QueueFull | Self::Unavailable | Self::NotFound | Self::BadSize => {
                true
            }
            Self::BadMagic | Self::BadHeader | Self::BadChecksum => false,
            Self::OutOfMemory | Self::NotPsram => true,
        }
    }
}

/// One queued complete-file stream. The channel carries no bytes, only identity.
///
/// The host harness compiles this module without the SD task owner, so these
/// APIs are unused there; the allowance is host-test-only and never hides
/// production dead code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) struct AssetReadRequest {
    pub(crate) id: u32,
    pub(crate) generation: u32,
}

/// Completion of one queued read, owned by whoever takes it.
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) struct AssetReadCompletion {
    pub(crate) id: u32,
    pub(crate) generation: u32,
    pub(crate) result: AssetReadOutcome,
}

static ASSET_REQUESTS: Channel<CriticalSectionRawMutex, AssetReadRequest, 1> = Channel::new();
static ASSET_RESULTS: Channel<CriticalSectionRawMutex, AssetReadCompletion, 1> = Channel::new();
/// Single-outstanding guard: set on successful queue, cleared when the
/// completion is taken (or force-released on the unexpected-full path).
static OUTSTANDING: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicU32 = AtomicU32::new(1);
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
static UPLOAD_GENERATION: AtomicU32 = AtomicU32::new(0);

#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) fn note_committed_upload() {
    UPLOAD_GENERATION.fetch_add(1, Ordering::Release);
}

#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) fn upload_generation() -> u32 {
    UPLOAD_GENERATION.load(Ordering::Acquire)
}

#[cfg(all(test, not(target_os = "none")))]
extern crate std;

/// Shared test-only serialization for the queue-lifecycle tests, both the
/// nested `tests` module below and the outer host integration tests that
/// include this file. One lock so parallel runners cannot interleave drains
/// and completions across the module boundary. Test-only; no production
/// static or DRAM change.
#[cfg(all(test, not(target_os = "none")))]
pub(crate) static QUEUE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Test-only reset: drain any queued request/completion and release the
/// single-outstanding guard. Queue tests must start from balanced state
/// because a passing test legitimately ends with the guard held (its final
/// `request_assets` proves the loader is reusable); without a reset the
/// suite is order-dependent. Test-only; no production use.
#[cfg(all(test, not(target_os = "none")))]
pub(crate) fn reset_loader_for_tests() {
    while ASSET_REQUESTS.try_receive().is_ok() {}
    while ASSET_RESULTS.try_receive().is_ok() {}
    OUTSTANDING.store(false, Ordering::Release);
}

/// Queue one complete-file asset stream. Non-blocking: returns
/// [`AssetReadError::Busy`] when a read is already outstanding instead of
/// waiting, so the UI never stalls the display loop while the SD worker may
/// be awaiting power on the shared I2C owner.
pub fn request_assets() -> Result<u32, AssetReadError> {
    if OUTSTANDING.swap(true, Ordering::AcqRel) {
        return Err(AssetReadError::Busy);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed).max(1);
    let generation = upload_generation();
    if ASSET_REQUESTS
        .try_send(AssetReadRequest { id, generation })
        .is_err()
    {
        OUTSTANDING.store(false, Ordering::Release);
        return Err(AssetReadError::QueueFull);
    }
    Ok(id)
}

/// Take the pending completion, if any. Taking releases the
/// single-outstanding flag, freeing the UI to queue a bounded retry after
/// any error. Dropping successful buffers frees their PSRAM; the active clock
/// screen owns it until navigation destroys that screen.
/// The full completion (including its request-matching id) is returned so
/// takers can log which transfer they reaped.
pub(crate) fn take_assets() -> Option<AssetReadCompletion> {
    let completion = ASSET_RESULTS.try_receive().ok()?;
    OUTSTANDING.store(false, Ordering::Release);
    Some(completion)
}

/// Non-blocking intake for host tests and drain helpers (the nested queue
/// tests and the outer harness integration tests). The firmware SD task
/// itself awaits [`receive_asset_request`], so this has no production user
/// and is compiled for host tests only instead of silencing a genuine
/// production dead-code warning.
#[cfg(all(test, not(target_os = "none")))]
pub(crate) fn poll_asset_request() -> Option<AssetReadRequest> {
    ASSET_REQUESTS.try_receive().ok()
}

/// Async intake for the SD task owner. Awaits the narrow request channel;
/// callers `select` this against the upload/core receives plus the idle
/// timer only — never against a `process_*`/handler future, which would
/// cancel an in-flight SD/DMA transfer.
///
/// Host-test-only allowance: the harness compiles this module without the
/// SD task owner.
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) async fn receive_asset_request() -> AssetReadRequest {
    ASSET_REQUESTS.receive().await
}

/// Publish one completion without blocking the SD task owner. The
/// completion is non-`Copy` (it may own the three PSRAM regions), so a full
/// channel returns ownership via `TrySendError::Full` and the owner is
/// recovered explicitly — never cloned, never sent with a blocking send.
/// A stale untaken result is dropped first (freeing its buffer); if the
/// fresh send still cannot land, the fresh buffer is dropped and the
/// single-outstanding guard released so a bounded retry stays possible
/// instead of deadlocking on `Busy`.
pub(crate) fn complete_asset_read(completion: AssetReadCompletion) {
    // Fast path: channel empty, fresh lands; OUTSTANDING stays set until
    // `take_assets` so the single-producer/one-outstanding contract holds.
    // On `Full`, recover ownership of `fresh` from the error (no Clone, no
    // blocking send), drop the stale queued reply to free its buffer, and
    // retry the fresh send once. Only if the retry still fails is `fresh`
    // dropped and OUTSTANDING released for a bounded retry.
    if let Err(TrySendError::Full(fresh)) = ASSET_RESULTS.try_send(completion) {
        if let Ok(stale) = ASSET_RESULTS.try_receive() {
            drop(stale);
        }
        if let Err(TrySendError::Full(fresh)) = ASSET_RESULTS.try_send(fresh) {
            drop(fresh);
            OUTSTANDING.store(false, Ordering::Release);
        }
    }
}

/// Builds the fixed `[u8; SD_PATH_MAX]` path field for the FAT read.
///
/// Host-test-only allowance: the harness compiles this module without the
/// SD task owner.
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) fn asset_path_field() -> ([u8; SD_PATH_MAX], u8) {
    let mut path = [0u8; SD_PATH_MAX];
    let len = ASSET_PATH.len().min(SD_PATH_MAX) as u8;
    path[..len as usize].copy_from_slice(&ASSET_PATH[..len as usize]);
    (path, len)
}

/// Validates one whole-file read by running the canonical
/// `clock-assets::decode` once over the buffer and mapping its error.
/// Rejects truncated, over-long, and corrupt files before the caller
/// borrows anything.
pub fn validate_assets(bytes: &[u8]) -> Result<(), AssetReadError> {
    ::clock_assets::decode(bytes)
        .map(|_| ())
        .map_err(map_decode_error)
}

pub(crate) fn map_decode_error(err: ::clock_assets::Error) -> AssetReadError {
    match err {
        ::clock_assets::Error::BadFileLen | ::clock_assets::Error::BadPayloadLen => {
            AssetReadError::BadSize
        }
        ::clock_assets::Error::BadMagic => AssetReadError::BadMagic,
        ::clock_assets::Error::BadLengthField | ::clock_assets::Error::BadReserved => {
            AssetReadError::BadHeader
        }
        ::clock_assets::Error::BadChecksum => AssetReadError::BadChecksum,
    }
}

pub(crate) fn core_mutation_invalidates_generation(command: &SdCommand) -> bool {
    fn touches_asset(path: &[u8; SD_PATH_MAX], path_len: u8) -> bool {
        path.get(..path_len as usize)
            .is_some_and(|used| used == ASSET_PATH)
    }
    match command {
        SdCommand::FatWrite { path, path_len, .. }
        | SdCommand::FatAppend { path, path_len, .. }
        | SdCommand::FatTruncate { path, path_len, .. }
        | SdCommand::FatRemove { path, path_len, .. } => touches_asset(path, *path_len),
        SdCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        } => touches_asset(src_path, *src_path_len) || touches_asset(dst_path, *dst_path_len),
        _ => false,
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    extern crate alloc;

    use super::*;

    #[test]
    fn queue_rejects_second_request_while_busy() {
        let _guard = QUEUE_LOCK.lock().unwrap();
        // Drain any leftover state from other tests; single-outstanding means
        // at most one request and one completion exist.
        while poll_asset_request().is_some() {}
        while take_assets().is_some() {}

        assert!(request_assets().is_ok());
        assert_eq!(request_assets(), Err(AssetReadError::Busy));
        // The first request is still queued exactly once.
        let queued = poll_asset_request().expect("request must be queued");
        assert_eq!(queued.generation, upload_generation());
        assert!(poll_asset_request().is_none());
        // No completion yet: still busy.
        assert!(take_assets().is_none());
        assert_eq!(request_assets(), Err(AssetReadError::Busy));

        // Simulate the SD worker completing with an error: taking it releases
        // the loader for a bounded retry.
        complete_asset_read(AssetReadCompletion {
            id: queued.id,
            generation: queued.generation,
            result: Err(AssetReadError::Unavailable),
        });
        let taken = take_assets().expect("completion must be present");
        assert_eq!(taken.id, queued.id);
        assert_eq!(taken.generation, queued.generation);
        assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::Unavailable));
        assert!(request_assets().is_ok());
        while poll_asset_request().is_some() {}
        OUTSTANDING.store(false, Ordering::Release);
    }

    #[test]
    fn validator_rejects_size_magic_and_reserved() {
        let mut file = alloc::vec![0u8; FILE_LEN];
        // Zeroed header: wrong magic first.
        assert_eq!(validate_assets(&file), Err(AssetReadError::BadMagic));
        file[0..8].copy_from_slice(&MAGIC);
        // Length field still zero.
        assert_eq!(validate_assets(&file), Err(AssetReadError::BadHeader));
        file[8..12].copy_from_slice(&(PAYLOAD_LEN as u32).to_le_bytes());
        // Payload is all zeros: CRC will not match zero field... set the
        // correct CRC, then break the reserved bytes.
        let sum = crc32(&file[HEADER_LEN..]);
        file[12..16].copy_from_slice(&sum.to_le_bytes());
        assert_eq!(validate_assets(&file), Ok(()));
        file[31] = 1;
        assert_eq!(validate_assets(&file), Err(AssetReadError::BadHeader));
        file[31] = 0;
        file[HEADER_LEN] ^= 0xFF;
        assert_eq!(validate_assets(&file), Err(AssetReadError::BadChecksum));
        // Truncation and over-long files never reach header parsing.
        assert_eq!(
            validate_assets(&file[..FILE_LEN - 1]),
            Err(AssetReadError::BadSize)
        );
    }

    #[test]
    fn stale_completion_replaced_and_fresh_delivered() {
        let _guard = QUEUE_LOCK.lock().unwrap();
        while poll_asset_request().is_some() {}
        while take_assets().is_some() {}

        // A stale untaken completion is dropped (freeing its buffer) so the
        // fresh honest result lands instead of wedging the loader.
        complete_asset_read(AssetReadCompletion {
            id: 7,
            generation: upload_generation(),
            result: Err(AssetReadError::Unavailable),
        });
        complete_asset_read(AssetReadCompletion {
            id: 8,
            generation: upload_generation(),
            result: Err(AssetReadError::NotFound),
        });
        let taken = take_assets().expect("fresh completion must be present");
        assert_eq!(taken.id, 8);
        assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::NotFound));
        // Taking releases the loader; no leftover completion remains.
        assert!(take_assets().is_none());
        assert!(request_assets().is_ok());
        while poll_asset_request().is_some() {}
        OUTSTANDING.store(false, Ordering::Release);
    }

    #[test]
    fn upload_generation_captured_per_request() {
        let _guard = QUEUE_LOCK.lock().unwrap();
        reset_loader_for_tests();

        let before = upload_generation();
        assert!(request_assets().is_ok());
        let first = poll_asset_request().expect("first request");
        assert_eq!(first.generation, before);
        assert!(take_assets().is_none());
        note_committed_upload();
        assert_eq!(upload_generation(), before.wrapping_add(1));
        complete_asset_read(AssetReadCompletion {
            id: first.id,
            generation: first.generation,
            result: Err(AssetReadError::Busy),
        });
        let taken = take_assets().expect("completion");
        assert_eq!(taken.generation, before);
        assert_ne!(taken.generation, upload_generation());
        assert!(request_assets().is_ok());
        let second = poll_asset_request().expect("second request");
        assert_eq!(second.generation, upload_generation());
        reset_loader_for_tests();
    }

    #[test]
    fn core_mutation_predicate_matches_only_the_clock_path() {
        use sdcard::request::SdCommand;
        use sdcard::SD_WRITE_MAX;

        fn other_field() -> ([u8; SD_PATH_MAX], u8) {
            let mut path = [0u8; SD_PATH_MAX];
            let other = b"/assets/AMBIENT/SKY.BIN";
            path[..other.len()].copy_from_slice(other);
            (path, other.len() as u8)
        }

        let (clock_path, clock_len) = asset_path_field();
        let (other_path, other_len) = other_field();
        let data = [0u8; SD_WRITE_MAX];

        assert!(core_mutation_invalidates_generation(&SdCommand::FatWrite {
            path: clock_path,
            path_len: clock_len,
            data,
            data_len: 0,
        }));
        assert!(core_mutation_invalidates_generation(
            &SdCommand::FatAppend {
                path: clock_path,
                path_len: clock_len,
                data,
                data_len: 0,
            }
        ));
        assert!(core_mutation_invalidates_generation(
            &SdCommand::FatTruncate {
                path: clock_path,
                path_len: clock_len,
                size: 0,
            }
        ));
        assert!(core_mutation_invalidates_generation(
            &SdCommand::FatRemove {
                path: clock_path,
                path_len: clock_len,
            }
        ));
        assert!(core_mutation_invalidates_generation(
            &SdCommand::FatRename {
                src_path: clock_path,
                src_path_len: clock_len,
                dst_path: other_path,
                dst_path_len: other_len,
            }
        ));
        assert!(core_mutation_invalidates_generation(
            &SdCommand::FatRename {
                src_path: other_path,
                src_path_len: other_len,
                dst_path: clock_path,
                dst_path_len: clock_len,
            }
        ));

        assert!(!core_mutation_invalidates_generation(&SdCommand::FatRead {
            path: clock_path,
            path_len: clock_len,
        }));
        assert!(!core_mutation_invalidates_generation(&SdCommand::FatStat {
            path: clock_path,
            path_len: clock_len,
        }));
        assert!(!core_mutation_invalidates_generation(&SdCommand::Probe));
        assert!(!core_mutation_invalidates_generation(
            &SdCommand::FatWrite {
                path: other_path,
                path_len: other_len,
                data,
                data_len: 0,
            }
        ));
        assert!(!core_mutation_invalidates_generation(
            &SdCommand::FatRemove {
                path: other_path,
                path_len: other_len,
            }
        ));
        assert!(!core_mutation_invalidates_generation(
            &SdCommand::FatRename {
                src_path: other_path,
                src_path_len: other_len,
                dst_path: other_path,
                dst_path_len: other_len,
            }
        ));
    }
}
