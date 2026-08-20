//! Host side of the `sd-qual-push` bench/qualification transport (ADR-0014
//! Phase 4) — see `src/updater/sd_push.rs`'s module doc for why this exists
//! and its wire protocol. Built on the same `SerialConsole` every other
//! hostctl serial workflow uses (regex line-waiting plus raw byte sends),
//! not a bespoke reader: the device only ever sends back text status lines,
//! and per-chunk pacing comes from waiting on those, not from any binary
//! ACK byte.

use std::{fs, path::PathBuf, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;

use crate::{env_utils, logging::Logger, serial_console::SerialConsole};

const MAGIC: &[u8; 4] = b"SDQP";
const DEFAULT_CHUNK_BYTES: usize = 8192;
const READY_TIMEOUT: Duration = Duration::from_secs(15);
/// Generous per-chunk margin: observed hardware timing is roughly 0.8-1.7s
/// per 8KB chunk (UART receive plus one SD write at the qual-push probe's
/// 400kHz SPI clock), depending on cluster-boundary FAT housekeeping.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(10);
const DONE_TIMEOUT: Duration = Duration::from_secs(15);

pub struct SdPushOptions {
    pub port: Option<String>,
    pub bundle_path: PathBuf,
    pub output: Option<PathBuf>,
}

/// Pushes `opts.bundle_path`'s bytes to a board already running the
/// `sd-qual-push` updater build variant, over one chunk-at-a-time,
/// wait-for-ack transfer. Returns an error (rather than hanging) on the
/// first missing/negative acknowledgment — the caller decides whether to
/// retry, matching every other hostctl workflow's failure contract.
pub fn run_sd_push(logger: &mut Logger, opts: SdPushOptions) -> Result<()> {
    let bundle = fs::read(&opts.bundle_path)
        .with_context(|| format!("failed to read bundle {}", opts.bundle_path.display()))?;
    if bundle.is_empty() {
        bail!("bundle {} is empty", opts.bundle_path.display());
    }

    let port = opts.port.map_or_else(env_utils::require_port, Ok)?;
    // Fixed, not env-configurable like most other workflows' baud: this
    // must match src/updater/sd_push.rs's UART0 baud exactly, and that
    // module deliberately never varies it (see its module doc for why a
    // mid-run baud switch was abandoned).
    let baud = 115_200;
    let mut console = SerialConsole::open(&port, baud, opts.output.as_deref())?;
    // Drain chunk acks promptly rather than waiting out a long default
    // per-read timeout on every poll — mirrors firmware_update's own
    // rationale for tightening this before a high-frequency ack loop.
    console.set_read_timeout(Duration::from_millis(2))?;

    let ready_mark = console.mark();
    let ready_re = Regex::new(r"UPDATER_SDPUSH_READY")?;
    console
        .wait_for_regex_since(ready_mark, &ready_re, READY_TIMEOUT)?
        .ok_or_else(|| anyhow!("device never reached UPDATER_SDPUSH_READY \u{2014} is it running the sd-qual-push build?"))?;
    logger.info("sd_push: device ready, sending header");

    let mut header = Vec::with_capacity(8);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&(bundle.len() as u32).to_le_bytes());
    console.send_bytes(&header)?;

    // Captured once, before the first chunk goes out, and reused for the
    // final UPDATER_SDPUSH_DONE wait below — not a fresh mark taken after
    // the loop. The device can print DONE immediately after the last
    // chunk's OK line, arriving in the same poll batch as that ack; a mark
    // taken only after the loop breaks can land *after* DONE was already
    // buffered, making wait_for_regex_since look for it in the wrong place
    // (observed on hardware: DONE was in the log, the host still timed
    // out). wait_for_regex_since scans every line since a mark, not just
    // new ones, so reusing an earlier mark is always safe.
    let transfer_mark = console.mark();
    let mut written = 0usize;
    let mut chunk_num = 0u32;
    while written < bundle.len() {
        let take = (bundle.len() - written).min(DEFAULT_CHUNK_BYTES);
        let mark = console.mark();
        console.send_bytes(&bundle[written..written + take])?;

        let ok_re = Regex::new(&format!(r"UPDATER_SDPUSH_CHUNK_OK n={chunk_num}\b"))?;
        let err_re = Regex::new(&format!(r"UPDATER_SDPUSH_CHUNK_ERROR n={chunk_num}\b"))?;
        let deadline = std::time::Instant::now() + CHUNK_TIMEOUT;
        loop {
            if let Some(line) = console.wait_for_regex_since(mark, &ok_re, Duration::from_millis(200))? {
                let _ = line;
                break;
            }
            if console.has_regex_since(mark, &err_re) {
                bail!("device reported UPDATER_SDPUSH_CHUNK_ERROR for chunk {chunk_num} (written={written})");
            }
            if std::time::Instant::now() >= deadline {
                bail!("chunk {chunk_num} was not acknowledged within {CHUNK_TIMEOUT:?} (written={written}/{})", bundle.len());
            }
        }

        written += take;
        chunk_num += 1;
        if chunk_num.is_multiple_of(16) || written == bundle.len() {
            logger.info(format!("sd_push: {written} / {} bytes sent", bundle.len()));
        }
    }

    let done_re = Regex::new(r"UPDATER_SDPUSH_DONE path=\S+ bytes=(\d+) ok=(true|false)")?;
    let done_line = console
        .wait_for_regex_since(transfer_mark, &done_re, DONE_TIMEOUT)?
        .ok_or_else(|| anyhow!("device never printed UPDATER_SDPUSH_DONE after the last chunk"))?;
    let captures = done_re
        .captures(&done_line)
        .expect("line matched the regex it was found by");
    let ok = &captures[2] == "true";
    let device_bytes: usize = captures[1].parse().unwrap_or(0);
    if !ok || device_bytes != bundle.len() {
        bail!("sd_push finished but the device reported ok={ok} bytes={device_bytes} (expected {})", bundle.len());
    }

    logger.info(format!(
        "sd_push complete: {} bytes written to the device's SD card",
        bundle.len()
    ));
    Ok(())
}
