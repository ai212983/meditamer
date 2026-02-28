use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
use regex::Regex;
use serde_json::{json, Value};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::{AckStatus, SerialConsole},
};

#[derive(Clone, Debug)]
pub struct RuntimeModesSmokeOptions {
    pub output_path: Option<PathBuf>,
}

fn open_console(output_path: &Path) -> Result<SerialConsole> {
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115200)?;
    ensure_parent_dir(output_path)?;
    SerialConsole::open(&port, baud, Some(output_path))
}

fn calc_local_tz_offset_minutes() -> i32 {
    Local::now().offset().local_minus_utc() / 60
}

fn query_mode_status(
    console: &mut SerialConsole,
    expect_upload: Option<&str>,
    expect_assets: Option<&str>,
) -> Result<String> {
    let pattern = Regex::new(r"STATE phase=.* base=.* upload=(on|off) assets=(on|off)")?;
    let mut line = None;
    for _ in 0..8 {
        let mark = console.mark();
        console.send_line("STATE GET")?;
        line = console.wait_for_regex_since(mark, &pattern, Duration::from_secs(4))?;
        if line.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    let line = line.ok_or_else(|| anyhow!("missing STATE GET response"))?;

    if let Some(expected) = expect_upload {
        if !line.contains(&format!("upload={expected}")) {
            return Err(anyhow!("STATE GET expected upload={expected}, got: {line}"));
        }
    }
    if let Some(expected) = expect_assets {
        if !line.contains(&format!("assets={expected}")) {
            return Err(anyhow!("STATE GET expected assets={expected}, got: {line}"));
        }
    }

    Ok(line)
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
    expect_assets: Option<&str>,
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
                return query_mode_status(console, expect_upload, expect_assets);
            }
            AckStatus::Busy | AckStatus::None => {
                thread::sleep(Duration::from_secs(1));
            }
            AckStatus::Err => {
                return Err(anyhow!(
                    "mode command returned error: {}",
                    line.unwrap_or_else(|| "STATE ERR".to_string())
                ));
            }
        }
    }
    Err(anyhow!("mode command failed after retries: {command}"))
}

fn run_timeset_probe(console: &mut SerialConsole, tz_offset_minutes: i32) -> Result<String> {
    let re = Regex::new(r"TIMESET (OK|BUSY)")?;
    for _ in 0..8 {
        let epoch = Utc::now().timestamp();
        let mark = console.mark();
        console.send_line(&format!("TIMESET {epoch} {tz_offset_minutes}"))?;
        if let Some(line) = console.wait_for_regex_since(mark, &re, Duration::from_secs(4))? {
            if line.contains("TIMESET OK") {
                return Ok(line);
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
    Err(anyhow!("timeset probe failed after retries"))
}

struct RuntimeModesScenarioRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    settle_ms: u64,
    post_upload_status_repeats: u32,
    post_upload_timeset_repeats: u32,
    mode_samples: Vec<String>,
    psram_samples: Vec<String>,
    timeset_samples: Vec<String>,
}

fn context_get_u32(context: &Value, key: &str) -> u32 {
    context
        .get(key)
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

fn context_set_u32(context: &mut Value, key: &str, value: u32) {
    if let Some(map) = context.as_object_mut() {
        map.insert(key.to_string(), Value::from(value));
    }
}

fn context_set_bool(context: &mut Value, key: &str, value: bool) {
    if let Some(map) = context.as_object_mut() {
        map.insert(key.to_string(), Value::from(value));
    }
}

impl<'a> RuntimeModesScenarioRuntime<'a> {
    fn new(
        logger: &'a mut Logger,
        console: SerialConsole,
        settle_ms: u64,
        post_upload_status_repeats: u32,
        post_upload_timeset_repeats: u32,
    ) -> Self {
        Self {
            logger,
            console,
            settle_ms,
            post_upload_status_repeats,
            post_upload_timeset_repeats,
            mode_samples: Vec::new(),
            psram_samples: Vec::new(),
            timeset_samples: Vec::new(),
        }
    }
}

impl WorkflowRuntime for RuntimeModesScenarioRuntime<'_> {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "state_get" => {
                let expect_upload = args.get("expect_upload").and_then(|v| v.as_str());
                let expect_assets = args.get("expect_assets").and_then(|v| v.as_str());
                let line = query_mode_status(&mut self.console, expect_upload, expect_assets)?;
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
                let expect_assets = args.get("expect_assets").and_then(|v| v.as_str());
                let line = apply_mode(
                    &mut self.console,
                    command,
                    expect_upload,
                    expect_assets,
                    self.settle_ms,
                )?;
                self.mode_samples.push(line);
                Ok(())
            }
            "init_post_upload_checks" => {
                if self.post_upload_status_repeats > 0 || self.post_upload_timeset_repeats > 0 {
                    self.logger
                        .info("Running post-upload UART regression checks...");
                }
                context_set_u32(context, "post_upload_status_index", 0);
                context_set_u32(context, "post_upload_timeset_index", 0);
                Ok(())
            }
            "set_post_upload_status_gate" => {
                let index = context_get_u32(context, "post_upload_status_index");
                context_set_bool(
                    context,
                    "run_post_upload_status_probe",
                    index < self.post_upload_status_repeats,
                );
                Ok(())
            }
            "run_post_upload_status_probe" => {
                let line = query_mode_status(&mut self.console, Some("on"), None)?;
                self.mode_samples.push(line);
                let next = context_get_u32(context, "post_upload_status_index").saturating_add(1);
                context_set_u32(context, "post_upload_status_index", next);
                Ok(())
            }
            "set_post_upload_timeset_gate" => {
                let index = context_get_u32(context, "post_upload_timeset_index");
                context_set_bool(
                    context,
                    "run_post_upload_timeset_probe",
                    index < self.post_upload_timeset_repeats,
                );
                Ok(())
            }
            "run_post_upload_timeset_probe" => {
                let tz_offset = calc_local_tz_offset_minutes();
                let probe_number = context_get_u32(context, "post_upload_timeset_index") + 1;
                let line = run_timeset_probe(&mut self.console, tz_offset)?;
                self.timeset_samples
                    .push(format!("timeset probe #{probe_number}: {line}"));
                context_set_u32(context, "post_upload_timeset_index", probe_number);
                Ok(())
            }
            "print_summary" => {
                self.logger.info("Mode responses:");
                for line in &self.mode_samples {
                    self.logger.info(format!("  {line}"));
                }
                self.logger.info("TIMESET probes:");
                for line in &self.timeset_samples {
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
}

pub fn run_runtime_modes_smoke(logger: &mut Logger, opts: RuntimeModesSmokeOptions) -> Result<()> {
    let settle_ms = env_utils::parse_env_u64("HOSTCTL_MODE_SMOKE_SETTLE_MS", 0)?;
    let post_upload_status_repeats =
        env_utils::parse_env_u32("HOSTCTL_MODE_SMOKE_POST_UPLOAD_STATUS_REPEATS", 3)?;
    let post_upload_timeset_repeats =
        env_utils::parse_env_u32("HOSTCTL_MODE_SMOKE_POST_UPLOAD_TIMESET_REPEATS", 2)?;

    let output_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(format!(
            "logs/runtime_modes_smoke_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ))
    });

    logger.info(format!(
        "Starting serial capture: {}",
        output_path.display()
    ));
    let console = open_console(&output_path)?;

    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/runtime-modes-smoke.sw.yaml"),
    )?;
    let mut runtime = RuntimeModesScenarioRuntime::new(
        logger,
        console,
        settle_ms,
        post_upload_status_repeats,
        post_upload_timeset_repeats,
    );

    let _ = execute_workflow(&workflow, &mut runtime, &json!({}))?;

    logger.info(format!(
        "Runtime mode smoke passed. Log: {}",
        output_path.display()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };

    use anyhow::{anyhow, Result};
    use serde_json::json;
    use serialport::TTYPort;
    use tempfile::tempdir;

    use super::RuntimeModesScenarioRuntime;
    use crate::{
        logging::Logger,
        scenarios::{execute_workflow, load_workflow},
        serial_console::SerialConsole,
    };

    fn open_pty_pair() -> Result<(TTYPort, TTYPort)> {
        TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))
    }

    #[test]
    fn runtime_modes_smoke_runs_against_fake_uart() -> Result<()> {
        let (mut master, slave) = open_pty_pair()?;

        let responder = thread::spawn(move || {
            let mut rx = Vec::<u8>::new();
            let mut chunk = [0u8; 512];
            let mut upload = "off".to_string();
            let mut assets = "on".to_string();
            let mut last_activity = Instant::now();

            loop {
                let n = match master.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        last_activity = Instant::now();
                        n
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                        if last_activity.elapsed() > Duration::from_secs(2) {
                            break;
                        }
                        continue;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        if last_activity.elapsed() > Duration::from_secs(2) {
                            break;
                        }
                        continue;
                    }
                    Err(_) => break,
                };
                rx.extend_from_slice(&chunk[..n]);

                while let Some(pos) = rx.iter().position(|b| *b == b'\n') {
                    let mut line = rx.drain(..=pos).collect::<Vec<u8>>();
                    while matches!(line.last(), Some(b'\r' | b'\n')) {
                        line.pop();
                    }
                    if line.is_empty() {
                        continue;
                    }
                    let command = String::from_utf8_lossy(&line).trim().to_string();
                    if command.is_empty() {
                        continue;
                    }

                    let response = if command == "STATE GET" {
                        format!("STATE phase=idle base=day upload={upload} assets={assets}")
                    } else if command == "STATE SET upload=on" {
                        upload = "on".to_string();
                        "STATE OK".to_string()
                    } else if command == "STATE SET upload=off" {
                        upload = "off".to_string();
                        "STATE OK".to_string()
                    } else if command == "STATE SET assets=off" {
                        assets = "off".to_string();
                        "STATE OK".to_string()
                    } else if command == "STATE SET assets=on" {
                        assets = "on".to_string();
                        "STATE OK".to_string()
                    } else if command == "PSRAM" {
                        "PSRAM feature_enabled=true state=ready total_bytes=1 used_bytes=1 free_bytes=0 peak_used_bytes=1"
                            .to_string()
                    } else {
                        String::new()
                    };

                    if !response.is_empty() {
                        let _ = master.write_all(response.as_bytes());
                        let _ = master.write_all(b"\r\n");
                        let _ = master.flush();
                    }
                }
            }
        });

        let temp = tempdir()?;
        let log_path = PathBuf::from(temp.path()).join("runtime_modes_fake_uart.log");
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/runtime-modes-smoke.sw.yaml");

        let mut logger = Logger::new(None)?;
        let workflow = load_workflow(&scenario_path)?;
        let console = SerialConsole::from_port_for_tests(Box::new(slave), Some(&log_path))?;
        let mut runtime = RuntimeModesScenarioRuntime::new(&mut logger, console, 0, 0, 0);
        let _ = execute_workflow(&workflow, &mut runtime, &json!({}))?;
        responder
            .join()
            .map_err(|_| anyhow!("fake UART responder thread panicked"))?;

        let raw = std::fs::read_to_string(log_path)?;
        if !raw.contains("STATE phase=idle") {
            return Err(anyhow!("runtime smoke capture missing STATE responses"));
        }
        if !raw.contains("PSRAM feature_enabled=true") {
            return Err(anyhow!("runtime smoke capture missing PSRAM responses"));
        }
        Ok(())
    }
}
