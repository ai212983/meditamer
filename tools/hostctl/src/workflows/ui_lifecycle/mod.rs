//! UI lifecycle regression run.
//!
//! Cycles the device through its surfaces over serial, captures the log, and
//! hands it to [`analysis`] to decide whether LVGL's allocator returned to the
//! same baseline each time. [`report`] holds the shape of that verdict.

mod analysis;
mod report;
#[cfg(test)]
mod tests;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, Result};
use chrono::Local;
use regex::Regex;
use serde_json::{json, Value};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::{AckStatus, SerialConsole},
};

const STEP_TIMEOUT: Duration = Duration::from_secs(180);
const READY_TIMEOUT: Duration = Duration::from_secs(120);

use analysis::analyze_lines;
use report::UiLifecycleReport;

#[derive(Clone, Debug)]
pub struct UiLifecycleOptions {
    pub cycles: u16,
    pub max_baseline_drift_bytes: usize,
    pub output_path: Option<PathBuf>,
}

fn report_path_for(log_path: &Path) -> PathBuf {
    let mut path = log_path.to_path_buf();
    path.set_extension("json");
    path
}

struct UiLifecycleRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    cycles: u16,
    max_baseline_drift_bytes: usize,
    evidence_mark: usize,
    log_path: PathBuf,
    report: Option<UiLifecycleReport>,
}

impl WorkflowRuntime for UiLifecycleRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, _context: &mut Value) -> Result<()> {
        match action {
            "await_ready" => {
                let ready = Regex::new(r"RUNTIME_READY app_state=ready display=ready")?;
                self.console
                    .wait_for_regex_since(0, &ready, READY_TIMEOUT)?
                    .ok_or_else(|| anyhow!("device did not report runtime ready"))?;
                Ok(())
            }
            "run_step" => {
                let mark = self.console.mark();
                self.console.send_line("UISTEP")?;
                let (status, line) = self.console.wait_ack_since(mark, "UISTEP", STEP_TIMEOUT)?;
                match status {
                    AckStatus::Ok => Ok(()),
                    AckStatus::None => Err(anyhow!(
                        "UISTEP timed out; outcome is ambiguous and the run stopped without retry"
                    )),
                    AckStatus::Busy | AckStatus::Err => Err(anyhow!(
                        "UISTEP failed: {}",
                        line.unwrap_or_else(|| "missing response".to_string())
                    )),
                }
            }
            "print_summary" => {
                let report = self
                    .report
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing report"))?;
                self.logger.info(format!(
                    "UI lifecycle passed: cycles={} steps={} report={}",
                    report.cycles_requested,
                    report.steps_expected,
                    report_path_for(&self.log_path).display()
                ));
                Ok(())
            }
            "fail_evidence" => {
                let report = self
                    .report
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing report"))?;
                Err(anyhow!(
                    "UI lifecycle evidence failed: {}",
                    report.violations.join("; ")
                ))
            }
            other => Err(anyhow!("unsupported ui-lifecycle action: {other}")),
        }
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        _args: &Value,
        _context: &mut Value,
    ) -> Result<Option<Value>> {
        match action {
            "init_run" => {
                self.evidence_mark = self.console.mark();
                Ok(Some(json!({
                    "step_count": usize::from(self.cycles) * 3,
                    "step_index": 0
                })))
            }
            "analyze_evidence" => {
                let lines = self.console.read_recent_lines(self.evidence_mark);
                let report = analyze_lines(&lines, self.cycles, self.max_baseline_drift_bytes);
                let run_passed = report.run_passed;
                let path = report_path_for(&self.log_path);
                ensure_parent_dir(&path)?;
                std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
                self.report = Some(report);
                Ok(Some(json!({ "run_passed": run_passed })))
            }
            _ => {
                self.invoke(action, _args, _context)?;
                Ok(None)
            }
        }
    }
}

pub fn run_ui_lifecycle(logger: &mut Logger, opts: UiLifecycleOptions) -> Result<()> {
    if !(2..=100).contains(&opts.cycles) {
        return Err(anyhow!("cycles must be in 2..=100"));
    }
    let log_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(format!(
            "logs/ui_lifecycle_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ))
    });
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115_200)?;
    ensure_parent_dir(&log_path)?;
    let console = SerialConsole::open(&port, baud, Some(&log_path))?;
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ui-lifecycle.sw.yaml"),
    )?;
    let mut runtime = UiLifecycleRuntime {
        logger,
        console,
        cycles: opts.cycles,
        max_baseline_drift_bytes: opts.max_baseline_drift_bytes,
        evidence_mark: 0,
        log_path,
        report: None,
    };
    let _ = execute_workflow(&workflow, &mut runtime, &json!({ "cycles": opts.cycles }))?;
    Ok(())
}
