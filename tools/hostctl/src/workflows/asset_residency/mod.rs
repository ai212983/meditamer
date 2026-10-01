//! Bounded asset-residency qualification.
//!
//! Drives already-flashed firmware over a resetting serial console through
//! two analog-clock visits or repeated Mountain/Clock re-entries, capturing
//! PSRAM snapshots at each residency transition. Scenario YAML owns orchestration, repetition, and
//! recovery; this module only implements atomic serial actions plus the
//! report finalizer.

mod line_checks;
mod marker_actions;
mod markers;
mod phase_actions;
mod report;
mod report_actions;
mod state_actions;
mod telemetry_actions;

use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use self::report::{report_path_for, AssetResidencyReport, CheckpointState};

use crate::{logging::Logger, scenarios::WorkflowRuntime, serial_console::SerialConsole};

const READY_POLL_BUDGET: usize = 120;
// First bounded Mountain SD job is non-cancellable and measured at roughly
// one minute, so serial state commands may be acknowledged only after it
// releases shared workflow resources; timeout remains finite.
const STATE_PROBE_TIMEOUT: Duration = Duration::from_secs(90);
const PSRAM_TIMEOUT: Duration = Duration::from_secs(8);
const UPLOAD_HTTP_PORT: u16 = 8080;
const MOUNTAIN_PACK_BYTES: u64 = 665_839;

/// Ordered required checkpoints. The finalizer fills unexecuted entries as
/// missing and passes only when every entry is present with status pass.
pub const REQUIRED_CHECKPOINTS: &[&str] = &[
    "ready",
    "ambient_assets_adopted",
    "mountain_validated",
    "mountain_rendered",
    "mountain_adopted",
    "mountain_composed",
    "ambient_ready",
    "clock1_entered",
    "clock_entry",
    "clock1_assets_ready",
    "clock_loaded",
    "clock1_frame_complete",
    "clock1_profile",
    "clock_first",
    "clock_steady_frame",
    "clock_steady",
    "clock1_assets_released",
    "clock_exit",
    "clock2_entered",
    "clock2_assets_ready",
    "clock2_frame_complete",
    "clock_reentry_first",
    "clock2_assets_released",
    "clock_reexit",
    "state_upload_on",
    "upload_on",
    "state_upload_off",
    "upload_off",
];

const REENTRY_CHECKPOINTS: &[&str] = &[
    "ambient_entered",
    "mountain_cache_validated",
    "mountain_validated",
    "mountain_rendered",
    "mountain_ui_status",
    "ambient_psram",
    "mountain_released",
    "mountain_exit_psram",
    "clock_entered",
    "clock_assets_ready",
    "clock_frame_complete",
    "clock_psram",
    "clock_released",
    "clock_exit_psram",
    "upload_on",
    "upload_on_psram",
    "upload_off",
    "upload_off_psram",
    "no_fault",
];

fn reentry_required_checkpoints(cycles: u8) -> Vec<String> {
    (1..=cycles)
        .flat_map(|cycle| {
            REENTRY_CHECKPOINTS
                .iter()
                .map(move |name| format!("cycle_{cycle:02}_{name}"))
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct AssetResidencyBaselineOptions {
    pub feature_label: String,
    pub build_label: String,
    pub output_path: Option<PathBuf>,
    pub marker_timeout_secs: u64,
    pub hold_samples: u8,
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

struct AssetResidencyRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    /// Consecutive `metrics_sample` calls with no `METRICS` terminal reply.
    /// A live device always answers the command itself even when its minima
    /// are missing, so a streak means the stream is dead, not slow.
    consecutive_misses: u32,
    /// Latest `net_liveness` one-line summary (`up <addr> rtt_ms=<n>`,
    /// `down <addr> err=<kind>`, or unknown). Quoted in the hang-abort
    /// error so a silent hang reads as UART-dead vs full stall.
    last_net: Option<String>,
    feature_label: String,
    build_label: String,
    marker_timeout: Duration,
    started_at: String,
    run_start: Instant,
    run_mark: usize,
    phase_name: String,
    phase_mark: usize,
    report_path: PathBuf,
    required_checkpoints: Vec<String>,
    active_cycle: Option<u8>,
    checkpoints: HashMap<String, CheckpointState>,
    state_lines: Vec<String>,
    report: Option<AssetResidencyReport>,
}

impl AssetResidencyRuntime<'_> {
    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.run_start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn record_marker(&mut self, checkpoint: &str, status: &str, line: Option<String>) {
        let elapsed_ms = self.elapsed_ms();
        let name = self.checkpoint_name(checkpoint);
        let entry = self.checkpoints.entry(name).or_default();
        entry.status = status.to_string();
        entry.elapsed_ms = Some(elapsed_ms);
        entry.marker_line = line;
    }

    fn checkpoint_name(&self, name: &str) -> String {
        match self.active_cycle {
            Some(cycle) => format!("cycle_{cycle:02}_{name}"),
            None => name.to_string(),
        }
    }

    fn search_mark(&self, from: &str) -> Result<usize> {
        match from {
            "run" => Ok(self.run_mark),
            "phase" => Ok(self.phase_mark),
            "fresh" => Ok(self.console.mark()),
            _ => Err(anyhow!(
                "await_marker requires from=run|phase|fresh, got `{from}`"
            )),
        }
    }
}

impl WorkflowRuntime for AssetResidencyRuntime<'_> {
    fn invoke(&mut self, action: &str, args: &Value, _context: &mut Value) -> Result<()> {
        if let Some(outcome) = self.dispatch_phase_actions(action, args) {
            return outcome;
        }
        if let Some(outcome) = self.dispatch_marker_actions(action, args) {
            return outcome;
        }
        if let Some(outcome) = self.dispatch_telemetry_actions(action, args) {
            return outcome;
        }
        if let Some(outcome) = self.dispatch_state_actions(action, args) {
            return outcome;
        }
        if let Some(outcome) = self.dispatch_report_actions(action, args) {
            return outcome;
        }
        Err(anyhow!("unsupported asset-residency action: {action}"))
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        match action {
            "init_ready" => Ok(Some(json!({
                "ready": false,
                "state_line": Value::Null,
                "upload": Value::Null,
                "ready_budget": vec![0u64; READY_POLL_BUDGET],
                "ready_index": 0,
            }))),
            "state_probe" => {
                let (ready, upload, line) = self.state_probe_impl(args)?;
                Ok(Some(json!({
                    "ready": ready,
                    "state_line": line,
                    "upload": upload,
                })))
            }
            "finalize_report" => {
                let run_passed = self.finalize_impl(context)?;
                Ok(Some(json!({ "run_passed": run_passed })))
            }
            _ => {
                self.invoke(action, args, context)?;
                Ok(None)
            }
        }
    }
}

mod runner;
pub use runner::{
    run_asset_residency_baseline, run_asset_residency_reentry, run_asset_residency_stress,
    run_stack_headroom_watch, run_stack_headroom_watch_upload_off,
    run_stack_headroom_watch_upload_on,
};

#[cfg(test)]
mod tests;
