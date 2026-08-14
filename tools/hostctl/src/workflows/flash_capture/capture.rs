use super::{CaptureMode, FlashResult, OutputPaths};
use std::{
    fs::{self, File},
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
) -> Result<Vec<u8>> {
    let mut console = SerialConsole::open(port, baud, None)?;
    console.clear_input_buffer().ok();
    console.pulse_en_reset(120, 20)?;
    let bytes = console.capture_raw_for(duration)?;
    fs::write(output_path, &bytes)?;
    Ok(bytes)
}

pub(super) fn capture_stream_window(
    port: &str,
    baud: u32,
    duration: Duration,
    output_path: &Path,
) -> Result<Vec<u8>> {
    let mut console = SerialConsole::open(port, baud, None)?;
    console.clear_input_buffer().ok();
    let bytes = console.capture_raw_for(duration)?;
    fs::write(output_path, &bytes)?;
    Ok(bytes)
}

pub(super) struct SummaryInputs<'a> {
    pub(super) outputs: &'a OutputPaths,
    pub(super) port: &'a str,
    pub(super) baud: u32,
    pub(super) flash_baud: u32,
    pub(super) result: &'a FlashResult,
    pub(super) capture_mode: CaptureMode,
    pub(super) capture_bytes: usize,
    pub(super) post_command: Option<&'a str>,
    pub(super) post_command_match: Option<&'a str>,
}

pub(super) fn write_summary(summary: SummaryInputs<'_>) -> Result<()> {
    let SummaryInputs {
        outputs,
        port,
        baud,
        flash_baud,
        result,
        capture_mode,
        capture_bytes,
        post_command,
        post_command_match,
    } = summary;
    let mut file = File::create(&outputs.summary)?;
    writeln!(file, "port={port}")?;
    writeln!(file, "baud={baud}")?;
    writeln!(file, "flash_baud={flash_baud}")?;
    writeln!(file, "strategy={:?}", result.strategy)?;
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
    writeln!(file, "reset_mode=en-only")?;
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
    Ok(())
}

fn display_opt_path(path: Option<&PathBuf>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "n/a".to_string())
}
