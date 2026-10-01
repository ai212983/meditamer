#![deny(unsafe_code)]

//! Narrow SD binary-frame loader intake for the Ambient Home sky/sun pack.
//!
//! Mirrors [`super::clock_assets`]: the UI worker queues at most one
//! outstanding whole-file read of `/assets/AMBIENT/SKY.BIN` (the
//! `ambient-assets` pack: 64-byte header + sky ink bits + clipped sun ink
//! and mask bits, `1` = ink/opaque) and polls for completion later. This
//! keeps the display task free to service shared-I2C SD power requests
//! while the SD worker owns the transfer: queue, return to the display
//! loop, service power, poll the result.
//!
//! The clipped sun size varies by pack. The loader checks the header and
//! allocates the exact format length in a strict-PSRAM runtime bundle. It
//! verifies the streamed count, header, and CRC before publication. The UI
//! owns the bundle while Ambient Home is active and drops it on exit. A
//! late completion is also dropped. Only request metadata crosses the
//! narrow channels; their depths are unchanged.
//!
//! See docs/references/memory/meditamer-inkplate/budget.md: no internal-DRAM bulk; the
//! request identity and generation counters stay small.

// Reuse the narrow loader taxonomy: the same retryable/non-retryable
// shape governs both asset packs, and a second enum would only drift.
pub use super::clock_assets::AssetReadError;

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

#[cfg(not(target_os = "none"))]
use super::super::psram::LargeByteBuffer;
use embassy_sync::channel::TrySendError;
#[cfg(target_os = "none")]
use phase_arena::RuntimeBundle;
use sdcard::request::SdCommand;
use sdcard::SD_PATH_MAX;

// Canonical single owner: `platform/ui/visuals/ambient-assets`. Absolute
// path (leading `::`) because the pack crate and this module share a name.
pub use ::ambient_assets::{crc32, HEADER_LEN, MAGIC, MAX_FILE_LEN, SKY_LEN};

/// SD path of the ambient sky/sun pack. It sits under the existing
/// `/assets` upload subtree, so the current HTTP upload service can stage
/// it with no new transfer mechanism.
pub const ASSET_PATH: &[u8] = b"/assets/AMBIENT/SKY.BIN";

/// Outcome of one whole-file asset read: the validated complete file in
/// strict PSRAM, or the reason the read failed.
#[cfg(target_os = "none")]
pub type AssetBuffer = RuntimeBundle<esp_alloc::ExternalMemory>;
#[cfg(not(target_os = "none"))]
pub type AssetBuffer = LargeByteBuffer;

pub type AssetReadOutcome = Result<AssetBuffer, AssetReadError>;

/// One queued whole-file read. The channel carries no bytes, only identity.
///
/// The host harness compiles this module without the SD task owner, so these
/// APIs are unused there; the allowance is host-test-only and never hides
/// production dead code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) struct AmbientReadRequest {
    pub(crate) id: u32,
    pub(crate) generation: u32,
}

/// Completion of one queued read, owned by whoever takes it.
///
/// A success carries the validated file length alongside its owner.
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) struct AmbientReadCompletion {
    pub(crate) id: u32,
    pub(crate) generation: u32,
    pub(crate) result: AssetReadOutcome,
    pub(crate) len: usize,
}

static ASSET_REQUESTS: Channel<CriticalSectionRawMutex, AmbientReadRequest, 1> = Channel::new();
static ASSET_RESULTS: Channel<CriticalSectionRawMutex, AmbientReadCompletion, 1> = Channel::new();
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
/// single-outstanding guard. Test-only; no production use.
///
/// Allowed dead: the host stub compiles this module into every harness
/// binary that shares the storage stub, but only the ambient intake binary
/// drains it. (The clock module's identical helper stays strict because
/// every baseline binary happens to drain it.)
#[cfg(all(test, not(target_os = "none")))]
#[allow(dead_code)]
pub(crate) fn reset_loader_for_tests() {
    while ASSET_REQUESTS.try_receive().is_ok() {}
    while ASSET_RESULTS.try_receive().is_ok() {}
    OUTSTANDING.store(false, Ordering::Release);
}

/// Queue one whole-file asset read. Non-blocking: returns
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
        .try_send(AmbientReadRequest { id, generation })
        .is_err()
    {
        OUTSTANDING.store(false, Ordering::Release);
        return Err(AssetReadError::QueueFull);
    }
    Ok(id)
}

/// Take the pending completion, if any. Taking releases the
/// single-outstanding flag, freeing the UI to queue a bounded retry after
/// any error. Dropping a successful buffer frees its PSRAM.
/// The full completion (including its request-matching id) is returned so
/// takers can log which transfer they reaped.
pub(crate) fn take_assets() -> Option<AmbientReadCompletion> {
    let completion = ASSET_RESULTS.try_receive().ok()?;
    OUTSTANDING.store(false, Ordering::Release);
    Some(completion)
}

/// Non-blocking intake for host tests and drain helpers. The firmware SD
/// task itself awaits [`receive_asset_request`], so this has no production
/// user and is compiled for host tests only instead of silencing a genuine
/// production dead-code warning.
#[cfg(all(test, not(target_os = "none")))]
pub(crate) fn poll_asset_request() -> Option<AmbientReadRequest> {
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
pub(crate) async fn receive_asset_request() -> AmbientReadRequest {
    ASSET_REQUESTS.receive().await
}

/// Publish one completion without blocking the SD task owner. The
/// completion is non-`Copy` (it may own the PSRAM buffer), so a full
/// channel returns ownership via `TrySendError::Full` and the owner is
/// recovered explicitly — never cloned, never sent with a blocking send.
/// A stale untaken result is dropped first (freeing its buffer); if the
/// fresh send still cannot land, the fresh buffer is dropped and the
/// single-outstanding guard released so a bounded retry stays possible
/// instead of deadlocking on `Busy`.
pub(crate) fn complete_asset_read(completion: AmbientReadCompletion) {
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
/// `ambient-assets::decode` once over the buffer and mapping its error.
/// Rejects truncated, over-long, and corrupt files before the caller
/// borrows anything.
pub fn validate_assets(bytes: &[u8]) -> Result<(), AssetReadError> {
    ::ambient_assets::decode(bytes)
        .map(|_| ())
        .map_err(map_decode_error)
}

pub(crate) fn map_decode_error(err: ::ambient_assets::Error) -> AssetReadError {
    match err {
        ::ambient_assets::Error::BadFileLen | ::ambient_assets::Error::BadPayloadLen => {
            AssetReadError::BadSize
        }
        ::ambient_assets::Error::BadMagic => AssetReadError::BadMagic,
        ::ambient_assets::Error::BadSkyGeometry
        | ::ambient_assets::Error::BadSunGeometry
        | ::ambient_assets::Error::BadLengthField
        | ::ambient_assets::Error::BadReserved => AssetReadError::BadHeader,
        ::ambient_assets::Error::BadChecksum => AssetReadError::BadChecksum,
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
    use super::*;

    #[test]
    fn queue_rejects_second_request_while_busy() {
        let _guard = QUEUE_LOCK.lock().unwrap();
        while poll_asset_request().is_some() {}
        while take_assets().is_some() {}

        let id = request_assets().expect("request must queue");
        assert_ne!(id, 0);
        assert_eq!(request_assets(), Err(AssetReadError::Busy));
        // The first request is still queued exactly once, carrying the
        // queued id and the current upload generation.
        let queued = poll_asset_request().expect("request must be queued");
        assert_eq!(queued.id, id);
        assert_eq!(queued.generation, upload_generation());
        assert!(poll_asset_request().is_none());
        // No completion yet: still busy.
        assert!(take_assets().is_none());
        assert_eq!(request_assets(), Err(AssetReadError::Busy));

        // Simulate the SD worker completing with an error: taking it releases
        // the loader for a bounded retry, echoing id and generation.
        complete_asset_read(AmbientReadCompletion {
            id: queued.id,
            generation: queued.generation,
            result: Err(AssetReadError::Unavailable),
            len: 0,
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
        complete_asset_read(AmbientReadCompletion {
            id: first.id,
            generation: first.generation,
            result: Err(AssetReadError::Busy),
            len: 0,
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
    fn core_mutation_predicate_matches_only_the_sky_path() {
        use sdcard::request::SdCommand;
        use sdcard::SD_WRITE_MAX;

        fn other_field() -> ([u8; SD_PATH_MAX], u8) {
            let mut path = [0u8; SD_PATH_MAX];
            let other = b"/assets/AMBIENT/MOUNTAIN.BIN";
            path[..other.len()].copy_from_slice(other);
            (path, other.len() as u8)
        }

        let (sky_path, sky_len) = asset_path_field();
        let (other_path, other_len) = other_field();
        let data = [0u8; SD_WRITE_MAX];

        assert!(core_mutation_invalidates_generation(&SdCommand::FatWrite {
            path: sky_path,
            path_len: sky_len,
            data,
            data_len: 0,
        }));
        assert!(core_mutation_invalidates_generation(
            &SdCommand::FatAppend {
                path: sky_path,
                path_len: sky_len,
                data,
                data_len: 0,
            }
        ));
        assert!(core_mutation_invalidates_generation(
            &SdCommand::FatTruncate {
                path: sky_path,
                path_len: sky_len,
                size: 0,
            }
        ));
        assert!(core_mutation_invalidates_generation(
            &SdCommand::FatRemove {
                path: sky_path,
                path_len: sky_len,
            }
        ));
        assert!(core_mutation_invalidates_generation(
            &SdCommand::FatRename {
                src_path: sky_path,
                src_path_len: sky_len,
                dst_path: other_path,
                dst_path_len: other_len,
            }
        ));
        assert!(core_mutation_invalidates_generation(
            &SdCommand::FatRename {
                src_path: other_path,
                src_path_len: other_len,
                dst_path: sky_path,
                dst_path_len: sky_len,
            }
        ));

        assert!(!core_mutation_invalidates_generation(&SdCommand::FatRead {
            path: sky_path,
            path_len: sky_len,
        }));
        assert!(!core_mutation_invalidates_generation(&SdCommand::FatStat {
            path: sky_path,
            path_len: sky_len,
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

    #[test]
    fn validator_maps_decode_errors() {
        extern crate alloc;
        // Too short to hold a header.
        assert_eq!(validate_assets(&[0u8; 16]), Err(AssetReadError::BadSize));
        // Zeroed max-size file: wrong magic first.
        let file = alloc::vec![0u8; MAX_FILE_LEN];
        assert_eq!(validate_assets(&file), Err(AssetReadError::BadMagic));
        // Minimal valid geometry (8x8 sun) with a wrong CRC, then fixed.
        let sun_len = 8usize;
        let payload_len = SKY_LEN + 2 * sun_len;
        let mut small = alloc::vec![0u8; ::ambient_assets::HEADER_LEN + payload_len];
        small[0..8].copy_from_slice(&::ambient_assets::MAGIC);
        small[8..10].copy_from_slice(&600u16.to_le_bytes());
        small[10..12].copy_from_slice(&600u16.to_le_bytes());
        small[12..14].copy_from_slice(&8u16.to_le_bytes());
        small[14..16].copy_from_slice(&8u16.to_le_bytes());
        small[16..20].copy_from_slice(&4.0f32.to_le_bytes());
        small[20..24].copy_from_slice(&4.0f32.to_le_bytes());
        small[24..28].copy_from_slice(&(SKY_LEN as u32).to_le_bytes());
        small[28..32].copy_from_slice(&(sun_len as u32).to_le_bytes());
        assert_eq!(validate_assets(&small), Err(AssetReadError::BadChecksum));
        let sum = ::ambient_assets::crc32(&small[::ambient_assets::HEADER_LEN..]);
        small[32..36].copy_from_slice(&sum.to_le_bytes());
        assert_eq!(validate_assets(&small), Ok(()));
        small[40] = 1;
        assert_eq!(validate_assets(&small), Err(AssetReadError::BadHeader));
        small[40] = 0;
        // Truncation and over-long files never reach payload parsing.
        assert_eq!(
            validate_assets(&small[..small.len() - 1]),
            Err(AssetReadError::BadSize)
        );
        let mut long = small.clone();
        long.extend_from_slice(&[0u8; 8]);
        assert_eq!(validate_assets(&long), Err(AssetReadError::BadSize));
    }
}
