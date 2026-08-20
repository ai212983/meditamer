//! Runtime SD installation (ADR-0014 Phase 3). `bundle_stream::stream_and_verify`
//! (pass 1) already checked the header, signature, and payload digest with
//! no flash writes at all — invariant 3, "the updater verifies the bundle
//! before erase." This module is pass 2: stream the same file again, this
//! time erasing-ahead-of-the-write-cursor into `ota_0` the same way
//! `src/firmware/update.rs`'s A/B `write_chunk` does, read the whole
//! written region back and hash it, and only then write the one activation
//! record invariant 2 calls for.
//!
//! Sequencing, and why each step is where it is:
//! 1. `attempted::record_attempted_digest` — invariant 4, *before* anything
//!    below touches flash.
//! 2. `crate::firmware::update::request_factory_boot()` — invariant 2's
//!    precondition ("both otadata sectors are erased"); also invariant 5's
//!    safety net, since a device power-cycled anywhere between here and
//!    step 5 has no otadata record pointing at `ota_0`'s possibly
//!    half-written contents, so it boots `factory` again next reset.
//! 3. Erase+write `ota_0` from the bundle's payload bytes.
//! 4. Full read-back compared against the header's digest — invariant 3,
//!    "verifies flash before activation."
//! 5. `activate` — invariant 2's "exactly one `NEW` record."
//!
//! A failure at any point before step 5 leaves both `otadata` sectors
//! erased and `ota_0` in whatever partial state the failure happened in —
//! harmless, since nothing selects it without a valid record.

use bundle::{BundleHeader, PayloadHasher};
use esp_hal::rtc_cntl::Rtc;
use sdcard::{
    fat::{FatEngine, FatIoCompletion, FatRequest, FatResult, FatStep},
    probe::{SdCardProbe, SdSpiBus},
};

use super::fat_io::{encode_path, execute_action};

/// Single-production layout (ADR-0014, config/partitions-single-production.csv).
const OTA_0_OFFSET: u32 = 0x80000;
const OTA_0_SIZE: u32 = 0x380000;
/// NOR flash erase granularity (matches `src/firmware/update.rs`'s
/// `OTA_DATA_SECTOR_SIZE`, which despite the name is just this board's
/// generic flash sector size, reused here for the same reason).
const FLASH_ERASE_SECTOR: u32 = 0x1000;
/// Keep ROM flash write calls no larger than this — the same bound
/// `src/firmware/update.rs::write_chunk` uses, "already proven on this
/// board" per its own comment.
const FLASH_PROGRAM_PAGE_BYTES: u32 = 256;
/// Read-back happens in bounded chunks too — no reason to hold more than
/// one SD sector's worth of flash content in RAM to hash it.
const READBACK_CHUNK_BYTES: usize = sdcard::probe::SD_SECTOR_SIZE;
/// Where a staged bundle goes once its digest is durably recorded as
/// attempted — see `retire_staged_bundle`'s doc for why this exists.
/// `replace: true` on the rename means a second, later attempt just
/// overwrites this rather than accumulating archived copies.
const RETIRED_PATH: &str = "/UPDATE.BIN.attempted";

// Read through the derived Debug impl in mod.rs's UPDATER_INSTALL_ERROR
// line, which rustc's dead_code analysis does not count as a use — same
// pattern as bundle_stream::BundleReadError.
#[allow(dead_code)]
#[derive(Debug)]
pub(super) enum InstallError {
    /// Only the factory updater may erase/write `ota_0` (invariant 1) —
    /// refuses to proceed if `status().booted` is not `Factory`.
    NotRunningFromFactory,
    Attempted(super::attempted::AttemptedError),
    RequestFactoryBoot,
    PathTooLong,
    Engine(sdcard::fat::FatEngineError),
    UnexpectedResult,
    Flash,
    /// The second streaming pass read fewer or more payload bytes than the
    /// header promised, or its digest no longer matches — the file changed
    /// between pass 1 and pass 2.
    PayloadChanged,
    ReadbackMismatch,
    Activate,
}

const fn align_up(value: u32, alignment: u32) -> u32 {
    value.saturating_add(alignment - 1) & !(alignment - 1)
}

/// Runs the full install sequence for an already-verified `header` staged
/// at `path`. On `Ok`, `ota_0` holds the verified image and is armed with
/// one `New` record — the caller's only remaining job is to reboot.
pub(super) async fn run<'d, SPI>(
    probe: &mut SdCardProbe<'d, SPI>,
    engine: &mut FatEngine,
    rtc: &mut Rtc<'static>,
    path: &str,
    header: &BundleHeader,
) -> Result<(), InstallError>
where
    SPI: SdSpiBus,
{
    let status =
        crate::firmware::update::status().map_err(|_| InstallError::NotRunningFromFactory)?;
    if status.booted != crate::firmware::update::Slot::Factory {
        return Err(InstallError::NotRunningFromFactory);
    }

    super::attempted::record_attempted_digest(probe, engine, &header.firmware_digest)
        .await
        .map_err(InstallError::Attempted)?;

    // Everything from here on is fallible, but `path` must not move until
    // every step that still reads it (write_payload_to_ota0's second
    // streaming pass, specifically) has run — retiring it only after
    // computing this result, not with `?` along the way, is what
    // guarantees that ordering regardless of which step fails.
    let result = install_from(probe, engine, rtc, path, header).await;

    // Best-effort, not load-bearing for correctness (the attempted-digest
    // marker already written above already blocks an automatic reinstall
    // of this exact bundle on its own) — but without it, every future
    // factory boot re-discovers this same file at `path` and pays a full
    // stream+hash+signature-verify pass just to re-derive the digest and
    // find it's already blocked. A stale bundle from an earlier, unrelated
    // session sitting at `path` is exactly how a prior hardware run on
    // this codebase produced a confusing accidental reinstall; retiring
    // the file once we've committed to attempting it — regardless of
    // whether that attempt went on to succeed — closes that gap. A rename
    // failure here does not change `result` — the marker above is what
    // actually has to be durable.
    retire_staged_bundle(probe, engine, path).await;

    result
}

async fn install_from<'d, SPI>(
    probe: &mut SdCardProbe<'d, SPI>,
    engine: &mut FatEngine,
    rtc: &mut Rtc<'static>,
    path: &str,
    header: &BundleHeader,
) -> Result<(), InstallError>
where
    SPI: SdSpiBus,
{
    crate::firmware::update::request_factory_boot()
        .map_err(|_| InstallError::RequestFactoryBoot)?;

    write_payload_to_ota0(
        probe,
        engine,
        rtc,
        path,
        header.firmware_len,
        &header.firmware_digest,
    )
    .await?;

    if !read_back_matches(rtc, header.firmware_len, &header.firmware_digest).await {
        return Err(InstallError::ReadbackMismatch);
    }

    activate().map_err(|_| InstallError::Activate)
}

/// Moves `path` (the bundle whose digest was just recorded as attempted) to
/// [`RETIRED_PATH`], `replace: true` so a later, unrelated attempt doesn't
/// pile up archived copies. Deliberately swallows every failure — see this
/// function's call site for why a failure here must not fail the install.
async fn retire_staged_bundle<'d, SPI>(
    probe: &mut SdCardProbe<'d, SPI>,
    engine: &mut FatEngine,
    path: &str,
) where
    SPI: SdSpiBus,
{
    let (Some((src_path, src_path_len)), Some((dst_path, dst_path_len))) =
        (encode_path(path), encode_path(RETIRED_PATH))
    else {
        return;
    };
    let Ok(()) = engine.start(FatRequest::Rename {
        src_path,
        src_path_len,
        dst_path,
        dst_path_len,
        replace: true,
    }) else {
        return;
    };
    let mut completion = FatIoCompletion::Pending;
    loop {
        match engine.advance(completion) {
            FatStep::Io(action) => completion = execute_action(action, probe, engine, &[]).await,
            FatStep::Continue | FatStep::Yield => completion = FatIoCompletion::Pending,
            FatStep::Complete(_) => return,
        }
    }
}

async fn write_payload_to_ota0<'d, SPI>(
    probe: &mut SdCardProbe<'d, SPI>,
    engine: &mut FatEngine,
    rtc: &mut Rtc<'static>,
    path: &str,
    expected_firmware_len: u32,
    expected_digest: &[u8; 32],
) -> Result<(), InstallError>
where
    SPI: SdSpiBus,
{
    let (path_bytes, path_len) = encode_path(path).ok_or(InstallError::PathTooLong)?;
    engine
        .start(FatRequest::Stream {
            path: path_bytes,
            path_len,
        })
        .map_err(InstallError::Engine)?;

    let mut completion = FatIoCompletion::Pending;
    let mut delivered = 0u32;
    let mut header_skipped = false;
    let mut flash_written = 0u32;
    let mut erased_until = 0u32;
    let mut hasher = PayloadHasher::new();

    loop {
        match engine.advance(completion) {
            FatStep::Io(action) => {
                completion = execute_action(action, probe, engine, &[]).await;
            }
            FatStep::Continue | FatStep::Yield => {
                let now = engine.stream_bytes_delivered();
                if now > delivered {
                    rtc.rwdt.feed();
                    let chunk_len = engine.stream_chunk_len() as usize;
                    let mut chunk = &engine.workspace().sector[..chunk_len];
                    if !header_skipped {
                        // The first chunk always contains the whole header
                        // (same reasoning as bundle_stream::stream_and_verify:
                        // HEADER_LEN < SD_SECTOR_SIZE, chunks are file-order).
                        chunk = &chunk[bundle::HEADER_LEN..];
                        header_skipped = true;
                    }
                    if !chunk.is_empty() {
                        hasher.update(chunk);
                        write_flash_chunk(chunk, flash_written, &mut erased_until).await?;
                        flash_written = flash_written.saturating_add(chunk.len() as u32);
                    }
                    delivered = now;
                }
                completion = FatIoCompletion::Pending;
            }
            FatStep::Complete(FatResult::Streamed { .. }) => {
                if flash_written != expected_firmware_len || !hasher.finish_matches(expected_digest)
                {
                    // Guards against the file changing on SD between pass 1
                    // (bundle_stream::stream_and_verify) and this pass — not
                    // the primary check for "did this land in flash
                    // correctly," which is read_back_matches re-reading from
                    // flash itself (invariant 3), but cheap insurance while
                    // the bytes are already in hand.
                    return Err(InstallError::PayloadChanged);
                }
                return Ok(());
            }
            FatStep::Complete(FatResult::Error(err)) => return Err(InstallError::Engine(err)),
            FatStep::Complete(_) => return Err(InstallError::UnexpectedResult),
        }
    }
}

async fn write_flash_chunk(
    chunk: &[u8],
    flash_offset: u32,
    erased_until: &mut u32,
) -> Result<(), InstallError> {
    let needed = align_up(flash_offset + chunk.len() as u32, FLASH_ERASE_SECTOR);
    let erase_to = needed.min(OTA_0_SIZE);
    if erase_to > *erased_until {
        crate::firmware::flash::erase(OTA_0_OFFSET + *erased_until, OTA_0_OFFSET + erase_to)
            .map_err(|_| InstallError::Flash)?;
        *erased_until = erase_to;
    }

    let mut batch_written = 0usize;
    while batch_written < chunk.len() {
        let absolute_offset = flash_offset + batch_written as u32;
        let page_remaining = FLASH_PROGRAM_PAGE_BYTES - absolute_offset % FLASH_PROGRAM_PAGE_BYTES;
        let segment_len = (chunk.len() - batch_written).min(page_remaining as usize);
        crate::firmware::flash::write(
            OTA_0_OFFSET + absolute_offset,
            &chunk[batch_written..batch_written + segment_len],
        )
        .map_err(|_| InstallError::Flash)?;
        batch_written += segment_len;
    }
    Ok(())
}

async fn read_back_matches(
    rtc: &mut Rtc<'static>,
    firmware_len: u32,
    expected_digest: &[u8; 32],
) -> bool {
    let mut hasher = PayloadHasher::new();
    let mut offset = 0u32;
    let mut buffer = [0u8; READBACK_CHUNK_BYTES];
    while offset < firmware_len {
        rtc.rwdt.feed();
        let len = (firmware_len - offset).min(READBACK_CHUNK_BYTES as u32) as usize;
        if crate::firmware::flash::read_aligned(OTA_0_OFFSET + offset, &mut buffer[..len]).is_err()
        {
            return false;
        }
        hasher.update(&buffer[..len]);
        offset += len as u32;
    }
    hasher.finish_matches(expected_digest)
}

/// Writes the one `New` record invariant 2 calls for. Both `otadata`
/// sectors are erased at this point (step 2 in `run`, untouched since), so
/// this is exactly the host-side `build_initial_otadata` sequence
/// (`tools/hostctl/src/workflows/single_production/otadata.rs`) — same
/// `ota_seq = 1`, same reasoning for why `1` and not the `0` a naive
/// multi-slot rotation would compute — except run on-device, where
/// `esp_bootloader_esp_idf`'s CRC uses the real ROM function
/// (`esp-bootloader-esp-idf`'s `rom` backend, selected automatically for
/// this target — no "std" feature here, unlike the host tool that needed a
/// hand-corrected `crc::Algorithm` to match it). Written directly rather
/// than through `Ota::set_current_app_partition`, which would recompute
/// that same `seq = 0` this file's whole module doc explains avoiding.
fn activate() -> Result<(), ()> {
    const OTA_DATA_OFFSET: u32 = 0xf000;
    const SEQUENCE: u32 = 1;

    let mut record = [0xFFu8; 32];
    record[..4].copy_from_slice(&SEQUENCE.to_le_bytes());
    record[24..28].copy_from_slice(&0u32.to_le_bytes()); // OtaImageState::New
    record[28..32].copy_from_slice(&crate::firmware::update::ota_crc(SEQUENCE).to_le_bytes());

    crate::firmware::flash::replace(OTA_DATA_OFFSET, &record).map_err(|_| ())
}
