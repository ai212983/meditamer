#![deny(unsafe_code)]

//! Narrow SD overlay-request intake for the Ambient Home mountain.
//!
//! The UI worker queues at most one outstanding overlay render of
//! `/assets/AMBIENT/MOUNTAIN.BIN` (the `mountain-snow` v1 pack: 64-byte
//! header + rock/snow gray rows, barrier rows, packed eligibility rows,
//! and blue-noise rows for the measured mountain-bounds band only) at one
//! snow-coverage percent and polls for completion later. This keeps the
//! display task free to service shared-I2C SD power requests while the SD
//! worker owns the transfer: queue, return to the display loop, service
//! power, poll the result.
//!
//! Intake contract (frozen 2026-09-23, extended 2026-09-25 with
//! percent/generation metadata): each request names the snow percent
//! (`0..=100`) and captures the current upload generation. The SD worker
//! owns a cached [`MountainSession`](super::sd_task::mountain_read::MountainSession)
//! per generation (validated header plus the 256-bin barrier histogram),
//! so a repeated percent reuses the session with no revalidation. When a
//! segmented external-PSRAM store is available, the SD task retains the
//! validated v1 pack while Ambient Home is active and later rows are local;
//! allocation failure retains the bounded five-SD-range fallback. Only
//! small request/completion metadata crosses the narrow
//! channels below plus the single overlay buffer owned by its completion.
//! Existing `SD_REQUESTS`/`SD_RESULTS` channels, depths, and serial buffers
//! are untouched. On success the UI retains the overlay while Ambient Home
//! is active and merges it into its existing 45 KB staging; an inactive UI
//! drops the taken result, freeing the buffer. Any error clears the
//! single-outstanding flag on `take`, so retry is always possible.
//!
//! See docs/references/memory/meditamer-inkplate/budget.md: no internal-DRAM bulk, no new
//! statics beyond channel metadata.

// Reuse the narrow loader taxonomy: the same retryable/non-retryable
// shape governs all asset packs, and a second enum would only drift.
pub use super::clock_assets::AssetReadError;

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

use super::super::psram::LargeByteBuffer;
use embassy_sync::channel::TrySendError;
use sdcard::request::SdCommand;
use sdcard::SD_PATH_MAX;

/// SD path of the mountain pack. It sits under the existing `/assets`
/// upload subtree, so the current HTTP upload service can stage it with
/// no new transfer mechanism.
pub const ASSET_PATH: &[u8] = b"/assets/AMBIENT/MOUNTAIN.BIN";
/// Exact v1 pack size in bytes (64-byte header + 269 stored rows). The SD
/// worker requires exactly this on validation before composing any row.
pub const MOUNTAIN_FILE_LEN: usize = 665_839;
/// First stored row (inclusive) and stored row count of the v1 pack.
pub const MOUNTAIN_FIRST_ROW: usize = 331;
pub const MOUNTAIN_ROWS: usize = 269;
/// Packed bytes of one overlay row (`600 / 8`); mirrors the ambient
/// composer constant so the SD worker and the UI agree on the layout
/// without a UI-to-storage dependency.
pub const OVERLAY_ROW_BYTES: usize = 75;
/// Packed bytes of one overlay plane (`ROWS * ROW_BYTES` = 20_175).
pub const OVERLAY_PLANE_BYTES: usize = MOUNTAIN_ROWS * OVERLAY_ROW_BYTES;
/// Total overlay bytes: `ROWS` packed ink rows immediately followed by
/// `ROWS` packed eligibility/opacity rows (40_350).
pub const OVERLAY_LEN: usize = 2 * OVERLAY_PLANE_BYTES;

/// Outcome of one overlay render: the composed strict-PSRAM overlay, or
/// the reason the render failed.
pub type AssetReadOutcome = Result<LargeByteBuffer, AssetReadError>;

/// One queued overlay render. The channel carries no bytes, only identity:
/// which snow percent to compose and which upload generation the request
/// was queued against.
///
/// The host harness compiles this module without the SD task owner, so these
/// APIs are unused there; the allowance is host-test-only and never hides
/// production dead code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) struct MountainReadRequest {
    pub(crate) id: u32,
    /// Snow coverage percent (`0..=100`) to compose.
    pub(crate) percent: u8,
    /// Upload generation captured at queue time (see [`upload_generation`]).
    pub(crate) generation: u32,
}

/// Completion of one queued render, owned by whoever takes it.
///
/// A success owns exactly one [`OVERLAY_LEN`]-byte buffer laid out as ink
/// plane then eligibility plane. The echoed `percent`/`generation` let the
/// UI drop stale completions (superseded percent or committed upload)
/// without counting a failure.
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) struct MountainReadCompletion {
    pub(crate) id: u32,
    pub(crate) percent: u8,
    pub(crate) generation: u32,
    pub(crate) result: AssetReadOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MountainCommand {
    Render(MountainReadRequest),
    ReleaseResident,
}

static ASSET_REQUESTS: Channel<CriticalSectionRawMutex, MountainCommand, 1> = Channel::new();
static ASSET_RESULTS: Channel<CriticalSectionRawMutex, MountainReadCompletion, 1> = Channel::new();
/// Single-outstanding guard: set on successful queue, cleared when the
/// completion is taken (or force-released on the unexpected-full path).
static OUTSTANDING: AtomicBool = AtomicBool::new(false);
static RESIDENT_WANTED: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicU32 = AtomicU32::new(1);
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
static UPLOAD_GENERATION: AtomicU32 = AtomicU32::new(0);

/// A committed replacement can rearm a parked missing/corrupt asset without
/// an unbounded retry timer. The SD owner calls this only after FAT commit.
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
#[cfg(all(test, not(target_os = "none")))]
#[allow(dead_code)]
pub(crate) fn reset_loader_for_tests() {
    while ASSET_REQUESTS.try_receive().is_ok() {}
    while ASSET_RESULTS.try_receive().is_ok() {}
    OUTSTANDING.store(false, Ordering::Release);
    RESIDENT_WANTED.store(false, Ordering::Release);
}

/// Queue one overlay render at `percent` snow coverage. Non-blocking:
/// returns [`AssetReadError::Busy`] when a render is already outstanding
/// instead of waiting, so the UI never stalls the display loop while the
/// SD worker may be awaiting power on the shared I2C owner.
///
/// Percents above 100 are rejected with [`AssetReadError::BadHeader`]
/// before the single-outstanding guard is set, so an invalid percent never
/// wedges the loader. The current [`upload_generation`] is captured into
/// the queued request so the SD worker can refuse a request the committed
/// upload superseded.
pub fn request_assets(percent: u8) -> Result<(), AssetReadError> {
    if percent > 100 {
        return Err(AssetReadError::BadHeader);
    }
    if OUTSTANDING.swap(true, Ordering::AcqRel) {
        return Err(AssetReadError::Busy);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed).max(1);
    let generation = upload_generation();
    RESIDENT_WANTED.store(true, Ordering::Release);
    if ASSET_REQUESTS
        .try_send(MountainCommand::Render(MountainReadRequest {
            id,
            percent,
            generation,
        }))
        .is_err()
    {
        OUTSTANDING.store(false, Ordering::Release);
        RESIDENT_WANTED.store(false, Ordering::Release);
        return Err(AssetReadError::QueueFull);
    }
    Ok(())
}

/// End the Ambient screen's pack lease. An idle SD owner wakes via the same
/// depth-one intake; if a render is queued, that render wakes it instead.
pub(crate) fn release_resident_pack() {
    if RESIDENT_WANTED.swap(false, Ordering::AcqRel) {
        let _ = ASSET_REQUESTS.try_send(MountainCommand::ReleaseResident);
    }
}

pub(crate) fn resident_wanted() -> bool {
    RESIDENT_WANTED.load(Ordering::Acquire)
}

/// Take the pending completion, if any. Taking releases the
/// single-outstanding flag, freeing the UI to queue a bounded retry after
/// any error. Dropping an overlay buffer frees its PSRAM; retaining it
/// keeps the composed planes while Ambient Home is active. The full
/// completion (including its request-matching id, percent, and generation)
/// is returned so takers can drop stale renders without counting a failure.
pub(crate) fn take_assets() -> Option<MountainReadCompletion> {
    let completion = ASSET_RESULTS.try_receive().ok()?;
    OUTSTANDING.store(false, Ordering::Release);
    Some(completion)
}

/// Non-blocking intake for host tests and drain helpers. The firmware SD
/// task itself awaits [`receive_asset_request`], so this has no production
/// user and is compiled for host tests only instead of silencing a genuine
/// production dead-code warning.
#[cfg(all(test, not(target_os = "none")))]
pub(crate) fn poll_asset_request() -> Option<MountainReadRequest> {
    match ASSET_REQUESTS.try_receive().ok()? {
        MountainCommand::Render(request) => Some(request),
        MountainCommand::ReleaseResident => None,
    }
}

/// Async intake for the SD task owner. Awaits the narrow request channel;
/// callers `select` this against the upload/core receives plus the idle
/// timer only — never against a `process_*`/handler future, which would
/// cancel an in-flight SD/DMA transfer.
///
/// Host-test-only allowance: the harness compiles this module without the
/// SD task owner.
#[cfg_attr(all(test, not(target_os = "none")), allow(dead_code))]
pub(crate) async fn receive_asset_request() -> MountainCommand {
    ASSET_REQUESTS.receive().await
}

/// Publish one completion without blocking the SD task owner. The
/// completion is non-`Copy` (it may own the overlay buffer), so a full
/// channel returns ownership via `TrySendError::Full` and the owner is
/// recovered explicitly — never cloned, never sent with a blocking send.
/// A stale untaken result is dropped first (freeing its buffer); if the
/// fresh send still cannot land, the fresh buffer is dropped and the
/// single-outstanding guard released so a bounded retry stays possible
/// instead of deadlocking on `Busy`.
pub(crate) fn complete_asset_read(completion: MountainReadCompletion) {
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

/// Snow-pixel target for `eligible_count` eligible pixels at `percent`
/// coverage, rounded to the nearest whole pixel. Same formula the ambient
/// composer uses before
/// [`cut_from_histogram`](mountain_snow::composer::cut_from_histogram), so
/// the SD worker's cached cut and the host composer agree bit for bit.
/// Callers only pass validated percents (`request_assets` rejects `> 100`,
/// and the composer rejects them again per row).
pub(crate) fn coverage_target(eligible_count: u32, percent: u8) -> u32 {
    (eligible_count * u32::from(percent) + 50) / 100
}

/// Whether a landed completion is still usable: its generation must equal
/// the current upload generation and its percent must still be desired.
/// Stale completions (a committed upload or a superseded percent) are
/// dropped without counting a failure. Pure so host tests can pin the
/// UI's stale-drop decision.
pub(crate) fn completion_is_usable(
    completion_generation: u32,
    completion_percent: u8,
    current_generation: u32,
    desired_percent: Option<u8>,
) -> bool {
    completion_generation == current_generation && Some(completion_percent) == desired_percent
}

/// Whether a queued request still describes the current file: a committed
/// upload between queue and serve supersedes it. The SD worker answers a
/// superseded request without touching SD or stale session metadata. Pure
/// so host tests can pin the serve-side generation gate.
pub(crate) fn request_is_current(request_generation: u32, current_generation: u32) -> bool {
    request_generation == current_generation
}

/// Whether the cached session may serve a current request without
/// revalidation: the request must be current and the session must carry the
/// same generation. Pure so host tests can pin the validate-once decision.
pub(crate) fn session_reusable(
    cached_generation: Option<u32>,
    request_generation: u32,
    current_generation: u32,
) -> bool {
    request_is_current(request_generation, current_generation)
        && cached_generation == Some(request_generation)
}

/// Whether a desired-percent edge must drop the retained overlay and rearm
/// the loader. A `Some(old)` -> `Some(new)` percent change always rearms;
/// an unavailable (`None`) -> `Some(new)` recovery rearms unless the
/// retained overlay is already exactly the current generation composed for
/// the recovered percent. A matching retained overlay survives the
/// unavailable round-trip (the loader queues nothing: the overlay is
/// already ready); `Some` -> `None` retains the old overlay and queues
/// nothing. Pure so host tests can pin the transition table; the owning
/// `SharedCache` (ambient view) performs the drop/rearm mutation.
pub(crate) fn transition_requires_rearm(
    previous_desired: Option<u8>,
    desired: Option<u8>,
    retained: Option<(u8, u32)>,
    current_generation: u32,
) -> bool {
    match (previous_desired, desired) {
        (Some(old), Some(new)) => old != new,
        (None, Some(new)) => retained != Some((new, current_generation)),
        _ => false,
    }
}

/// Whether a successful core (non-upload) FAT mutation touches the mountain
/// pack path, so the SD owner must bump the upload generation and drop the
/// cached session/overlay. Only the exact [`ASSET_PATH`] counts, on the
/// mutated path and (for rename) on either side; reads and unrelated paths
/// never invalidate. Callers bump only after a successful commit — never on
/// a failed operation. Pure so host tests can pin the path table.
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
#[path = "mountain_assets/tests.rs"]
mod tests;
