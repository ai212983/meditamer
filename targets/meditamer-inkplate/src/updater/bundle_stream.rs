//! Drives a [`sdcard::fat::FatRequest::Stream`] over the staged bundle,
//! verifying the header as soon as it is available (invariant 3: "the
//! updater verifies the bundle before erase") and hashing the firmware
//! payload as it streams past, without ever holding the whole bundle in
//! memory (ADR-0014 Phase 1: "Read and hash a complete bundle from SD with
//! bounded buffers").

use bundle::{BundleError, BundleHeader, PayloadHasher, HEADER_LEN};
use ed25519_dalek::VerifyingKey;
use sdcard::{
    fat::{FatEngine, FatEngineError, FatIoCompletion, FatRequest, FatResult, FatStep},
    probe::{SdCardProbe, SdSpiBus},
};

use super::fat_io::{encode_path, execute_action};

// Every variant's payload is read through the derived `Debug` impl in
// status::print_bundle_error, which rustc's dead_code analysis does not
// count as a use.
#[allow(dead_code)]
#[derive(Debug)]
pub(super) enum BundleReadError {
    /// `path` does not fit in `SD_PATH_MAX`.
    PathTooLong,
    /// The FAT engine reported a failure (not-found, transport, etc.).
    Engine(FatEngineError),
    /// The stream completed with a `FatResult` this driver never issues.
    UnexpectedResult,
    /// The file was empty; there was no header to read at all.
    Empty,
    /// The header failed to parse or verify.
    Bundle(BundleError),
    /// The header verified, but the streamed payload's digest did not match.
    DigestMismatch,
}

pub(super) struct VerifiedBundle {
    pub(super) header: BundleHeader,
    pub(super) total_bytes: u32,
}

/// Streams `path` off `probe`/`engine`, parses+verifies the leading
/// [`HEADER_LEN`]-byte [`BundleHeader`] against `expected_target_id` /
/// `expected_layout_id` / `max_firmware_len` / `public_key`, and hashes
/// every payload byte after it. Returns as soon as verification fails —
/// there is no reason to keep streaming (and no reason to erase anything)
/// once the bundle is known bad.
///
/// Relies on `HEADER_LEN` (144 bytes) being smaller than one SD sector (512
/// bytes): the header is always fully contained in the first chunk
/// `FatRequest::Stream` delivers, since chunks are file-order and the first
/// one is `min(SD_SECTOR_SIZE, file_size)` bytes starting at offset 0.
pub(super) async fn stream_and_verify<'d, SPI>(
    probe: &mut SdCardProbe<'d, SPI>,
    engine: &mut FatEngine,
    path: &str,
    expected_target_id: u16,
    expected_layout_id: u16,
    max_firmware_len: u32,
    public_key: &VerifyingKey,
) -> Result<VerifiedBundle, BundleReadError>
where
    SPI: SdSpiBus,
{
    const _: () = assert!(HEADER_LEN <= sdcard::probe::SD_SECTOR_SIZE);

    let (path_bytes, path_len) = encode_path(path).ok_or(BundleReadError::PathTooLong)?;
    engine
        .start(FatRequest::Stream {
            path: path_bytes,
            path_len,
        })
        .map_err(BundleReadError::Engine)?;

    let mut completion = FatIoCompletion::Pending;
    let mut delivered = 0u32;
    let mut header: Option<BundleHeader> = None;
    let mut hasher = PayloadHasher::new();

    loop {
        match engine.advance(completion) {
            FatStep::Io(action) => {
                completion = execute_action(action, probe, engine, &[]).await;
            }
            FatStep::Continue | FatStep::Yield => {
                let now = engine.stream_bytes_delivered();
                if now > delivered {
                    let chunk_len = engine.stream_chunk_len() as usize;
                    let chunk = &engine.workspace().sector[..chunk_len];
                    match &header {
                        None => {
                            // A file shorter than one header can't have a
                            // header at all — fail closed instead of
                            // panicking on the short slice below. See
                            // BundleError::Truncated for the >= HEADER_LEN
                            // case decode() itself still needs to check
                            // (a multi-chunk file's first chunk is always
                            // exactly SD_SECTOR_SIZE, comfortably >= 144).
                            if chunk.len() < HEADER_LEN {
                                return Err(BundleReadError::Bundle(BundleError::Truncated));
                            }
                            let mut header_bytes = [0u8; HEADER_LEN];
                            header_bytes.copy_from_slice(&chunk[..HEADER_LEN]);
                            let parsed = BundleHeader::decode(&header_bytes)
                                .map_err(BundleReadError::Bundle)?;
                            parsed
                                .verify(
                                    expected_target_id,
                                    expected_layout_id,
                                    max_firmware_len,
                                    public_key,
                                )
                                .map_err(BundleReadError::Bundle)?;
                            hasher.update(&chunk[HEADER_LEN..]);
                            header = Some(parsed);
                        }
                        Some(_) => hasher.update(chunk),
                    }
                    delivered = now;
                }
                completion = FatIoCompletion::Pending;
            }
            FatStep::Complete(FatResult::Streamed { bytes }) => {
                let header = header.ok_or(BundleReadError::Empty)?;
                if !hasher.finish_matches(&header.firmware_digest) {
                    return Err(BundleReadError::DigestMismatch);
                }
                return Ok(VerifiedBundle {
                    header,
                    total_bytes: bytes,
                });
            }
            FatStep::Complete(FatResult::Error(err)) => return Err(BundleReadError::Engine(err)),
            FatStep::Complete(_) => return Err(BundleReadError::UnexpectedResult),
        }
    }
}
