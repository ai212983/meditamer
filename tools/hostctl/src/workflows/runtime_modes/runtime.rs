use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::{json, Value};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::WorkflowRuntime,
    serial_console::{AckStatus, SerialConsole},
};

#[derive(Clone, Debug)]
pub struct RuntimeModesSmokeOptions {
    pub output_path: Option<PathBuf>,
    pub suite: String,
}

pub(super) fn open_console(output_path: &Path) -> Result<SerialConsole> {
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115200)?;
    ensure_parent_dir(output_path)?;
    SerialConsole::open(&port, baud, Some(output_path))
}

fn query_mode_status(
    console: &mut SerialConsole,
    expect_upload: Option<&str>,
    expect_ready: bool,
) -> Result<String> {
    let pattern = Regex::new(r"^STATE phase=.* upload=(on|off).* ready=(true|false)$")?;
    let mut last_line = None;
    // Opening the UART resets the board. The first response can therefore be
    // delayed by the current synchronous full e-paper boot render.
    for _ in 0..24 {
        let mark = console.mark();
        console.send_line("STATE GET")?;
        if let Some(line) = console.wait_for_regex_since(mark, &pattern, Duration::from_secs(4))? {
            let upload_matches = expect_upload
                .map(|expected| line.contains(&format!("upload={expected}")))
                .unwrap_or(true);
            let ready_matches = !expect_ready || line.contains("ready=true");
            if upload_matches && ready_matches {
                return Ok(line);
            }
            last_line = Some(line);
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(anyhow!(
        "STATE GET did not converge (upload={:?} ready={}): {}",
        expect_upload,
        expect_ready,
        last_line.unwrap_or_else(|| "<no response>".to_string())
    ))
}

fn capture_psram_snapshot(console: &mut SerialConsole) -> Result<String> {
    let mark = console.mark();
    console.send_line("PSRAM")?;
    let pattern = Regex::new(r"PSRAM feature_enabled=")?;
    let line = console
        .wait_for_regex_since(mark, &pattern, Duration::from_secs(8))?
        .ok_or_else(|| anyhow!("missing PSRAM response"))?;
    Ok(line)
}

fn apply_mode(
    console: &mut SerialConsole,
    command: &str,
    expect_upload: Option<&str>,
    expect_ready: bool,
    settle_ms: u64,
) -> Result<String> {
    for _ in 0..8 {
        let mark = console.mark();
        console.send_line(command)?;
        let (status, line) = console.wait_ack_since(mark, "STATE", Duration::from_secs(4))?;
        match status {
            AckStatus::Ok => {
                if settle_ms > 0 {
                    thread::sleep(Duration::from_millis(settle_ms));
                }
                return query_mode_status(console, expect_upload, expect_ready);
            }
            AckStatus::Busy | AckStatus::None => {
                thread::sleep(Duration::from_secs(1));
            }
            AckStatus::Err => {
                if line
                    .as_deref()
                    .is_some_and(|line| line.contains("reason=timeout"))
                {
                    if let Ok(status) = query_mode_status(console, expect_upload, expect_ready) {
                        return Ok(status);
                    }
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
                return Err(anyhow!(
                    "mode command returned error: {}",
                    line.unwrap_or_else(|| "STATE ERR".to_string())
                ));
            }
        }
    }
    Err(anyhow!("mode command failed after retries: {command}"))
}

fn run_ping_probe(console: &mut SerialConsole) -> Result<String> {
    let re = Regex::new(r"^PONG$")?;
    for _ in 0..8 {
        let mark = console.mark();
        console.send_line("PING")?;
        if let Some(line) = console.wait_for_regex_since(mark, &re, Duration::from_secs(4))? {
            return Ok(line);
        }
        thread::sleep(Duration::from_secs(1));
    }
    Err(anyhow!("ping probe failed after retries"))
}

pub(super) struct RuntimeModesScenarioRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    settle_ms: u64,
    post_upload_status_repeats: u32,
    post_upload_ping_repeats: u32,
    mode_samples: Vec<String>,
    psram_samples: Vec<String>,
    ping_samples: Vec<String>,
}

fn context_get_u32(context: &Value, key: &str) -> u32 {
    context
        .get(key)
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

impl<'a> RuntimeModesScenarioRuntime<'a> {
    pub(super) fn new(
        logger: &'a mut Logger,
        console: SerialConsole,
        settle_ms: u64,
        post_upload_status_repeats: u32,
        post_upload_ping_repeats: u32,
    ) -> Self {
        Self {
            logger,
            console,
            settle_ms,
            post_upload_status_repeats,
            post_upload_ping_repeats,
            mode_samples: Vec::new(),
            psram_samples: Vec::new(),
            ping_samples: Vec::new(),
        }
    }

    fn build_post_upload_checks_result(&mut self) -> Value {
        if self.post_upload_status_repeats > 0 || self.post_upload_ping_repeats > 0 {
            self.logger
                .info("Running post-upload UART regression checks...");
        }
        json!({
            "post_upload_status_repeats": self.post_upload_status_repeats,
            "post_upload_ping_repeats": self.post_upload_ping_repeats,
            "post_upload_status_index": 0,
            "post_upload_ping_index": 0
        })
    }
}

impl WorkflowRuntime for RuntimeModesScenarioRuntime<'_> {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "state_get" => {
                let expect_upload = args.get("expect_upload").and_then(|v| v.as_str());
                let expect_ready = args
                    .get("expect_ready")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let line = query_mode_status(&mut self.console, expect_upload, expect_ready)?;
                self.mode_samples.push(line);
                Ok(())
            }
            "psram_snapshot" => {
                let label = args
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("snapshot");
                let line = capture_psram_snapshot(&mut self.console)?;
                self.psram_samples.push(format!("{label}: {line}"));
                Ok(())
            }
            "apply_mode" => {
                let command = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("apply_mode requires command"))?;
                let expect_upload = args.get("expect_upload").and_then(|v| v.as_str());
                let expect_ready = args
                    .get("expect_ready")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let line = apply_mode(
                    &mut self.console,
                    command,
                    expect_upload,
                    expect_ready,
                    self.settle_ms,
                )?;
                self.mode_samples.push(line);
                Ok(())
            }
            "init_post_upload_checks" => {
                let _ = self.build_post_upload_checks_result();
                Ok(())
            }
            "run_post_upload_status_probe" => {
                let line = query_mode_status(&mut self.console, Some("on"), true)?;
                self.mode_samples.push(line);
                Ok(())
            }
            "run_post_upload_ping_probe" => {
                let probe_number = context_get_u32(context, "post_upload_ping_index") + 1;
                let line = run_ping_probe(&mut self.console)?;
                self.ping_samples
                    .push(format!("ping probe #{probe_number}: {line}"));
                Ok(())
            }
            "print_summary" => {
                self.logger.info("Mode responses:");
                for line in &self.mode_samples {
                    self.logger.info(format!("  {line}"));
                }
                self.logger.info("PING probes:");
                for line in &self.ping_samples {
                    self.logger.info(format!("  {line}"));
                }
                self.logger.info("PSRAM snapshots:");
                for line in &self.psram_samples {
                    self.logger.info(format!("  {line}"));
                }
                Ok(())
            }
            other => Err(anyhow!("unsupported runtime-modes action: {other}")),
        }
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        if action == "init_post_upload_checks" {
            return Ok(Some(self.build_post_upload_checks_result()));
        }
        self.invoke(action, args, context)?;
        Ok(None)
    }
}
