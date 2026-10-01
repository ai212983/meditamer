use super::{CaptureMode, FlashResult, OutputPaths};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Result;

use crate::serial_console::SerialConsole;

pub(super) fn capture_boot_window(
    port: &str,
    baud: u32,
    duration: Duration,
    output_path: &Path,
) -> Result<(Vec<u8>, SerialConsole)> {
    let mut console = SerialConsole::open(port, baud, None)?;
    console.clear_input_buffer().ok();
    console.pulse_en_reset(120, 20)?;
    let bytes = console.capture_raw_for(duration)?;
    fs::write(output_path, &bytes)?;
    Ok((bytes, console))
}

/// Looks for an already-captured `TIME_REQUEST` line in a boot/stream
/// capture log (see `docs/references/runtime/serial-control.md#time-synchronization`).
/// The device prints it once, very early in its serial task, so a
/// capture that ran long enough to see boot at all should have seen it too
/// -- this lets `action_time_sync` reply to a session the capture step
/// already observed, instead of trying (and failing) to watch for it live
/// on a fresh console that opens well after the device stopped waiting to
/// be heard for the first time. `None` for `capture_mode: "none"` (nothing
/// was captured) or if the line simply isn't there -- either way the caller
/// falls back to a live wait.
pub(super) fn find_captured_request(log_path: &Path) -> Option<wall_clock::message::SyncRequest> {
    let bytes = fs::read(log_path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // The last one, in the unlikely event more than one appears in one
    // capture (e.g. a device that reset mid-window) -- that is the session
    // still open by the time this runs.
    text.lines()
        .rev()
        .find_map(wall_clock::codec::decode_request)
}

pub(super) fn append_time_sync_diagnostics(log_path: &Path, lines: &[String]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    for line in lines.iter().filter(|line| {
        line.starts_with("RTC_DIAG ")
            || line.starts_with("I2C_PROXY_TIMEOUT ")
            || line.starts_with("I2C_PROXY_OWNER ")
            || line.starts_with("I2C_TIMEOUT ")
            || line.starts_with("I2C_ADMISSION_ERROR ")
            || line.starts_with("TIME_SYNC ")
    }) {
        writeln!(file, "{line}")?;
    }
    file.flush()?;
    Ok(())
}

pub(super) fn capture_stream_window(
    port: &str,
    baud: u32,
    duration: Duration,
    output_path: &Path,
) -> Result<(Vec<u8>, SerialConsole)> {
    let mut console = SerialConsole::open(port, baud, None)?;
    console.clear_input_buffer().ok();
    let bytes = console.capture_raw_for(duration)?;
    fs::write(output_path, &bytes)?;
    Ok((bytes, console))
}

pub(super) struct SummaryInputs<'a> {
    pub(super) outputs: &'a OutputPaths,
    pub(super) port: &'a str,
    pub(super) baud: u32,
    pub(super) flash_baud: u32,
    pub(super) result: &'a FlashResult,
    pub(super) boot_target: &'a str,
    pub(super) capture_mode: CaptureMode,
    pub(super) capture_bytes: usize,
    pub(super) post_command: Option<&'a str>,
    pub(super) post_command_match: Option<&'a str>,
    pub(super) time_sync_status: &'a str,
    pub(super) time_sync_requested_utc: Option<u32>,
    pub(super) time_sync_requested_offset_minutes: Option<i16>,
    pub(super) time_sync_utc: Option<u32>,
    pub(super) time_sync_offset_minutes: Option<i16>,
    pub(super) time_sync_reason: Option<&'a str>,
}

pub(super) fn write_summary(summary: SummaryInputs<'_>) -> Result<()> {
    let SummaryInputs {
        outputs,
        port,
        baud,
        flash_baud,
        result,
        boot_target,
        capture_mode,
        capture_bytes,
        post_command,
        post_command_match,
        time_sync_status,
        time_sync_requested_utc,
        time_sync_requested_offset_minutes,
        time_sync_utc,
        time_sync_offset_minutes,
        time_sync_reason,
    } = summary;
    let mut file = File::create(&outputs.summary)?;
    writeln!(file, "port={port}")?;
    writeln!(file, "baud={baud}")?;
    writeln!(file, "flash_baud={flash_baud}")?;
    writeln!(file, "strategy={:?}", result.strategy)?;
    writeln!(file, "boot_target={boot_target}")?;
    writeln!(file, "capture_mode={:?}", capture_mode)?;
    writeln!(file, "capture_bytes={capture_bytes}")?;
    writeln!(file, "image_path={}", result.image_path.display())?;
    writeln!(file, "fallback_used={}", result.fallback_used)?;
    writeln!(
        file,
        "python_bin={}",
        display_opt_path(result.python_bin.as_ref())
    )?;
    writeln!(
        file,
        "idf_root={}",
        display_opt_path(result.idf_root.as_ref())
    )?;
    writeln!(
        file,
        "idf_py_bin={}",
        display_opt_path(result.idf_py_bin.as_ref())
    )?;
    writeln!(
        file,
        "reset_mode={}",
        if capture_mode == CaptureMode::Boot {
            "en-only"
        } else {
            "esptool-hard-reset"
        }
    )?;
    writeln!(file, "firmware_elf={}", outputs.firmware_elf.display())?;
    writeln!(file, "app_bin={}", outputs.app_bin.display())?;
    writeln!(file, "sha256={}", outputs.hashes.display())?;
    writeln!(file, "build_metadata={}", outputs.build_metadata.display())?;
    writeln!(file, "post_command={}", post_command.unwrap_or("n/a"))?;
    writeln!(
        file,
        "post_command_match={}",
        post_command_match.unwrap_or("n/a")
    )?;
    writeln!(file, "time_sync={time_sync_status}")?;
    writeln!(
        file,
        "time_sync_requested_utc={}",
        display_opt(time_sync_requested_utc)
    )?;
    writeln!(
        file,
        "time_sync_requested_offset_min={}",
        display_opt(time_sync_requested_offset_minutes)
    )?;
    writeln!(file, "time_sync_utc={}", display_opt(time_sync_utc))?;
    writeln!(
        file,
        "time_sync_offset_min={}",
        display_opt(time_sync_offset_minutes)
    )?;
    writeln!(
        file,
        "time_sync_reason={}",
        time_sync_reason.unwrap_or("n/a")
    )?;
    Ok(())
}

fn display_opt<T: std::fmt::Display>(value: Option<T>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn display_opt_path(path: Option<&PathBuf>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "n/a".to_string())
}
