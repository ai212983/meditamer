//! Flash-and-capture workflow.
//!
//! [`runtime`] and [`workflow_runtime`] drive the scenario; [`flash`] and
//! [`capture`] are the two halves it alternates between; [`command_run`] and
//! [`command_relay`] run external tools; [`paths`] and [`artifacts`] place the
//! output. The options and command types they share live here.

mod artifacts;
mod capture;
mod command_relay;
mod command_run;
mod flash;
mod paths;
mod runtime;
mod runtime_capture;
mod runtime_helpers;
#[cfg(test)]
mod tests;
mod workflow_runtime;

pub use runtime::run_flash_capture;

use std::{
    ffi::OsString,
    fs::File,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use clap::ValueEnum;
use fs2::FileExt;

use crate::{idf_env::IdfEnv, logging::Logger};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum FlashMode {
    Auto,
    Full,
    AppOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CaptureMode {
    Boot,
    Stream,
    None,
}

#[derive(Debug)]
pub struct FlashCaptureOptions {
    pub profile: String,
    pub output_path: Option<PathBuf>,
    pub port: Option<String>,
    pub flash_mode: FlashMode,
    pub capture_mode: CaptureMode,
    pub image: Option<PathBuf>,
    pub flash_baud: Option<u32>,
    pub baud: Option<u32>,
    pub boot_window_ms: Option<u64>,
    pub idf_root: Option<PathBuf>,
    pub idf_tools_path: Option<PathBuf>,
    pub post_command: Option<String>,
    pub post_pattern: Option<String>,
    pub post_timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    program: OsString,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
    env_remove: Vec<OsString>,
}

impl CommandSpec {
    pub(super) fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            current_dir: None,
            env_remove: Vec::new(),
        }
    }

    pub(super) fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub(super) fn args<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(values.into_iter().map(Into::into));
        self
    }

    pub(super) fn current_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(dir.into());
        self
    }

    pub(super) fn env_remove(mut self, name: impl Into<OsString>) -> Self {
        self.env_remove.push(name.into());
        self
    }
}

pub(super) struct PortLock {
    file: File,
}

impl Drop for PortLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

pub(super) struct OutputPaths {
    root: PathBuf,
    flash_log: PathBuf,
    capture_log: PathBuf,
    post_command_log: PathBuf,
    summary: PathBuf,
    firmware_elf: PathBuf,
    app_bin: PathBuf,
    hashes: PathBuf,
    build_metadata: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FlashStrategy {
    Full,
    AppOnly,
}

pub(super) struct FlashResult {
    strategy: FlashStrategy,
    image_path: PathBuf,
    fallback_used: bool,
    python_bin: Option<PathBuf>,
    idf_root: Option<PathBuf>,
    idf_py_bin: Option<PathBuf>,
}

pub(super) struct FlashCaptureRuntime<'a> {
    logger: &'a mut Logger,
    opts: FlashCaptureOptions,
    repo_dir: PathBuf,
    outputs: OutputPaths,
    port: String,
    _port_lock: Option<PortLock>,
    baud: u32,
    flash_baud: u32,
    fallback_baud: u32,
    flash_timeout: Duration,
    flash_status_interval: Duration,
    flash_idle_timeout: Duration,
    flash_progress_stall_timeout: Duration,
    flash_log_drain_timeout: Duration,
    boot_window: Duration,
    skip_update_check: bool,
    image_path: Option<PathBuf>,
    idf_env: Option<IdfEnv>,
    flash_result: Option<FlashResult>,
    capture_bytes: usize,
    post_command_match: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandProgressMode {
    EsptoolWriteFlash,
}

const DEFAULT_LOG_DRAIN_TIMEOUT: Duration = Duration::from_millis(1_000);
const DEFAULT_ENABLE_FALLBACK: bool = false;
pub const DEFAULT_FLASH_BAUD: u32 = 115_200;

#[derive(Debug)]
pub(super) struct LogRelayThread {
    id: usize,
    handle: thread::JoinHandle<Result<()>>,
}

#[derive(Debug)]
pub(super) struct CommandMonitorState {
    last_output_at: Instant,
    last_progress_at: Option<Instant>,
    last_progress_marker: Option<String>,
}

impl CommandMonitorState {
    pub(super) fn new() -> Self {
        Self {
            last_output_at: Instant::now(),
            last_progress_at: None,
            last_progress_marker: None,
        }
    }

    pub(super) fn note_output(&mut self) {
        self.last_output_at = Instant::now();
    }

    pub(super) fn note_progress(&mut self, marker: String) {
        self.last_output_at = Instant::now();
        if self.last_progress_marker.as_deref() == Some(marker.as_str()) {
            return;
        }
        self.last_progress_at = Some(Instant::now());
        self.last_progress_marker = Some(marker);
    }
}
