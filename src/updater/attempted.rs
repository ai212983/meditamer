//! Tracks the digest of the last install attempt on SD (invariant 4,
//! docs/plans/single-production-sd-recovery-updater.md: "The updater
//! records an attempted digest before activation. Reinstalling it requires
//! a new explicit request, even when no `ABORTED` record survives"). A
//! marker file on the card that already holds the bundle keeps this
//! concern scoped to "what has this card's candidate already tried,"
//! rather than mixed into production firmware's own `app_state` partition
//! (a different owner, a different concern) or RTC memory (which does not
//! survive a full power-on reset/brownout — only deep-sleep wake and
//! software resets; see the "deep sleep is planned" project note — while a
//! power interruption mid-install is exactly the case this exists for).

use sdcard::{
    fat::{FatEngine, FatIoCompletion, FatPayloadId, FatRequest, FatResult, FatStep},
    probe::{SdCardProbe, SdSpiBus},
};

use super::fat_io::{encode_path, execute_action};

const MARKER_PATH: &str = "/UPDATE.ATTEMPTED";
const DIGEST_LEN: usize = 32;

// Read through the derived Debug impl in mod.rs's UPDATER_INSTALL_ERROR
// line, which rustc's dead_code analysis does not count as a use — same
// pattern as bundle_stream::BundleReadError.
#[allow(dead_code)]
#[derive(Debug)]
pub(super) enum AttemptedError {
    PathTooLong,
    Engine(sdcard::fat::FatEngineError),
    UnexpectedResult,
}

/// Reads the marker file's recorded digest, if any. A missing file (never
/// attempted anything on this card) and a present-but-unreadable file both
/// come back as `None` — either way there is nothing to block on, so a
/// caller does not need to distinguish "not found" from "one weird
/// filesystem error reading 32 bytes."
pub(super) async fn read_attempted_digest<'d, SPI>(
    probe: &mut SdCardProbe<'d, SPI>,
    engine: &mut FatEngine,
) -> Option<[u8; DIGEST_LEN]>
where
    SPI: SdSpiBus,
{
    let (path, path_len) = encode_path(MARKER_PATH)?;
    engine.start(FatRequest::Stream { path, path_len }).ok()?;

    let mut completion = FatIoCompletion::Pending;
    let mut delivered = 0u32;
    let mut digest = [0u8; DIGEST_LEN];
    let mut filled = 0usize;

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
                    let take = chunk.len().min(DIGEST_LEN - filled);
                    digest[filled..filled + take].copy_from_slice(&chunk[..take]);
                    filled += take;
                    delivered = now;
                }
                completion = FatIoCompletion::Pending;
            }
            FatStep::Complete(FatResult::Streamed { .. }) => {
                return if filled == DIGEST_LEN {
                    Some(digest)
                } else {
                    None
                };
            }
            FatStep::Complete(_) => return None,
        }
    }
}

/// Writes `digest` to the marker file, replacing whatever was there before.
/// Must happen — and be durable — before `install::run` erases anything
/// (invariant 4: recorded *before* activation, not after).
pub(super) async fn record_attempted_digest<'d, SPI>(
    probe: &mut SdCardProbe<'d, SPI>,
    engine: &mut FatEngine,
    digest: &[u8; DIGEST_LEN],
) -> Result<(), AttemptedError>
where
    SPI: SdSpiBus,
{
    let (path, path_len) = encode_path(MARKER_PATH).ok_or(AttemptedError::PathTooLong)?;
    engine
        .start(FatRequest::Write {
            path,
            path_len,
            input: FatPayloadId::Primary,
            input_len: DIGEST_LEN as u32,
        })
        .map_err(AttemptedError::Engine)?;

    let mut completion = FatIoCompletion::Pending;
    loop {
        match engine.advance(completion) {
            FatStep::Io(action) => {
                completion = execute_action(action, probe, engine, digest).await;
            }
            FatStep::Continue | FatStep::Yield => completion = FatIoCompletion::Pending,
            FatStep::Complete(FatResult::Done) => return Ok(()),
            FatStep::Complete(FatResult::Error(err)) => return Err(AttemptedError::Engine(err)),
            FatStep::Complete(_) => return Err(AttemptedError::UnexpectedResult),
        }
    }
}

/// Whether `candidate_digest` (the bundle currently staged) has already been
/// attempted according to the marker — the condition that blocks an
/// automatic reinstall.
pub(super) fn blocks_reinstall(
    attempted: Option<[u8; DIGEST_LEN]>,
    candidate_digest: &[u8; DIGEST_LEN],
) -> bool {
    attempted.is_some_and(|recorded| &recorded == candidate_digest)
}
