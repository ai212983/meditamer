use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use regex::Regex;
use serde_json::{json, Value};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::{AckStatus, SerialConsole},
};

#[derive(Clone, Debug)]
pub enum SdcardSuite {
    All,
    Baseline,
    Burst,
    Failures,
}

fn suite_name(suite: &SdcardSuite) -> &'static str {
    match suite {
        SdcardSuite::All => "all",
        SdcardSuite::Baseline => "baseline",
        SdcardSuite::Burst => "burst",
        SdcardSuite::Failures => "failures",
    }
}

#[derive(Clone, Debug)]
pub struct SdcardHwOptions {
    pub build_mode: String,
    pub output_path: Option<PathBuf>,
    pub suite: SdcardSuite,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn open_console(output_path: &Path) -> Result<SerialConsole> {
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115200)?;
    ensure_parent_dir(output_path)?;
    SerialConsole::open(&port, baud, Some(output_path))
}

fn maybe_flash_first(logger: &mut Logger, build_mode: &str) -> Result<()> {
    let flash_first = env_utils::parse_env_bool01("HOSTCTL_SDCARD_FLASH_FIRST", false)?;
    if !flash_first {
        return Ok(());
    }

    logger.info(format!(
        "Flashing firmware ({build_mode}) before SD-card test..."
    ));
    let port = env_utils::require_port()?;
    let repo_dir = repo_root();
    let status = Command::new(repo_root().join("scripts/device/flash.sh"))
        .arg(build_mode)
        .current_dir(&repo_dir)
        .env_remove("RUSTUP_TOOLCHAIN")
        .env("ESPFLASH_PORT", port)
        .env("FLASH_SET_TIME_AFTER_FLASH", "0")
        .status()
        .context("failed to execute scripts/device/flash.sh")?;

    if !status.success() {
        return Err(anyhow!("flash.sh failed with status: {status}"));
    }
    Ok(())
}

fn wait_for_sd_result(
    console: &mut SerialConsole,
    request_id: u32,
    timeout_ms: u32,
    expected_status: &str,
    expected_code: Option<&str>,
) -> Result<()> {
    let line = console
        .sdwait_for_id(request_id, timeout_ms)?
        .ok_or_else(|| anyhow!("missing SDWAIT response"))?;

    if !line.contains("SDWAIT DONE") && expected_status != "timeout" {
        return Err(anyhow!("unexpected SDWAIT response: {line}"));
    }

    let status_re = Regex::new(r"status=([a-z]+)")?;
    let code_re = Regex::new(r"code=([a-z0-9_]+)")?;
    let mut status = status_re
        .captures(&line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "-".to_string());
    let mut code = code_re
        .captures(&line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "-".to_string());

    if status == "-" || code == "-" {
        let done_prefix = Regex::new(&format!(r"^SDDONE id={} ", request_id))?;
        if let Some(done_line) = console.last_regex_since(0, &done_prefix) {
            if status == "-" {
                status = status_re
                    .captures(&done_line)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or(status);
            }
            if code == "-" {
                code = code_re
                    .captures(&done_line)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or(code);
            }
        }
    }

    if status != expected_status {
        return Err(anyhow!("expected status={expected_status}, got {line}"));
    }
    if let Some(expected_code) = expected_code {
        if code != expected_code {
            return Err(anyhow!("expected code={expected_code}, got {line}"));
        }
    }

    Ok(())
}

fn force_upload_mode_off(logger: &mut Logger, console: &mut SerialConsole) -> Result<()> {
    for _ in 0..12 {
        let mark = console.mark();
        console.send_line("STATE SET upload=off")?;
        let (status, line) = console.wait_ack_since(mark, "STATE", Duration::from_secs(4))?;
        match status {
            AckStatus::Ok => {
                logger.info("Precondition: upload mode forced off");
                return Ok(());
            }
            AckStatus::Busy | AckStatus::None => {
                thread::sleep(Duration::from_secs(1));
            }
            AckStatus::Err => {
                if line
                    .as_deref()
                    .is_some_and(|msg| msg.contains("reason=timeout"))
                {
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
                return Err(anyhow!(
                    "failed forcing upload mode off before SD suite: {}",
                    line.unwrap_or_else(|| "STATE ERR".to_string())
                ));
            }
        }
    }

    logger.warn("Could not confirm upload mode off before SD suite; proceeding");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_step(
    logger: &mut Logger,
    console: &mut SerialConsole,
    name: &str,
    command: &str,
    ack_tag: &str,
    expected_status: &str,
    expected_code: Option<&str>,
    expected_pattern: Option<&Regex>,
    timeout_ms: u32,
) -> Result<()> {
    for _ in 0..12 {
        let mark = console.mark();
        console.send_line(command)?;
        let (status, line) = console.wait_ack_since(mark, ack_tag, Duration::from_secs(8))?;

        match status {
            AckStatus::Busy | AckStatus::None => {
                thread::sleep(Duration::from_secs(2));
                continue;
            }
            AckStatus::Err => {
                return Err(anyhow!("{name} failed: {}", line.unwrap_or_default()));
            }
            AckStatus::Ok => {
                let req_id = console
                    .wait_for_sdreq_id_since(mark, None, Duration::from_secs(8))?
                    .ok_or_else(|| anyhow!("{name}: missing SDREQ id"))?;
                wait_for_sd_result(console, req_id, timeout_ms, expected_status, expected_code)?;

                if let Some(pattern) = expected_pattern {
                    let matched = console
                        .wait_for_regex_since(mark, pattern, Duration::from_secs(90))?
                        .is_some();
                    if !matched {
                        return Err(anyhow!("{name}: missing expected completion marker"));
                    }
                }

                logger.info(format!("[PASS] {name}"));
                return Ok(());
            }
        }
    }

    Err(anyhow!("[FAIL] {name}"))
}

fn run_raw_expect_pattern(
    logger: &mut Logger,
    console: &mut SerialConsole,
    name: &str,
    command: &str,
    expected_pattern: &Regex,
    timeout: Duration,
) -> Result<()> {
    let mark = console.mark();
    console.send_line(command)?;
    let line = console.wait_for_regex_since(mark, expected_pattern, timeout)?;
    if line.is_none() {
        return Err(anyhow!("[FAIL] {name}: missing expected pattern"));
    }
    logger.info(format!("[PASS] {name}"));
    Ok(())
}

fn required_arg_str<'a>(args: &'a Value, key: &str, action: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!("{action} requires string argument '{key}'"))
}

fn optional_arg_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(|value| value.as_u64())
        .and_then(|raw| u32::try_from(raw).ok())
}

pub(crate) fn resolve_templates(template: &str, vars: &HashMap<String, String>) -> Result<String> {
    let mut output = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let remaining = &rest[start + 1..];
        let end = remaining
            .find('}')
            .ok_or_else(|| anyhow!("unclosed template placeholder in '{template}'"))?;
        let key = remaining[..end].trim();
        if key.is_empty() {
            return Err(anyhow!("empty template placeholder in '{template}'"));
        }
        let replacement = vars
            .get(key)
            .ok_or_else(|| anyhow!("unknown template variable '{{{key}}}'"))?;
        output.push_str(replacement);
        rest = &remaining[end + 1..];
    }

    output.push_str(rest);
    Ok(output)
}

struct SdcardScenarioRuntime<'a> {
    logger: &'a mut Logger,
    console: &'a mut SerialConsole,
    vars: HashMap<String, String>,
    sdwait_timeout_ms: u32,
    burst_mark: Option<usize>,
}

impl SdcardScenarioRuntime<'_> {
    fn resolve(&self, raw: &str) -> Result<String> {
        resolve_templates(raw, &self.vars)
    }

    fn invoke_run_step(&mut self, args: &Value) -> Result<()> {
        let name = self.resolve(required_arg_str(args, "name", "run_step")?)?;
        let command = self.resolve(required_arg_str(args, "command", "run_step")?)?;
        let ack_tag = required_arg_str(args, "ack_tag", "run_step")?;
        let expected_status = required_arg_str(args, "expected_status", "run_step")?;
        let expected_code = args.get("expected_code").and_then(|value| value.as_str());
        let timeout_ms = optional_arg_u32(args, "timeout_ms").unwrap_or(self.sdwait_timeout_ms);

        let expected_pattern = if let Some(raw_pattern) = args
            .get("expected_pattern")
            .and_then(|value| value.as_str())
        {
            Some(Regex::new(&self.resolve(raw_pattern)?)?)
        } else {
            None
        };

        run_step(
            self.logger,
            self.console,
            &name,
            &command,
            ack_tag,
            expected_status,
            expected_code,
            expected_pattern.as_ref(),
            timeout_ms,
        )
    }

    fn invoke_raw_expect_pattern(&mut self, args: &Value) -> Result<()> {
        let name = self.resolve(required_arg_str(args, "name", "raw_expect_pattern")?)?;
        let command = self.resolve(required_arg_str(args, "command", "raw_expect_pattern")?)?;
        let expected_pattern = Regex::new(&self.resolve(required_arg_str(
            args,
            "expected_pattern",
            "raw_expect_pattern",
        )?)?)?;
        let timeout_ms = optional_arg_u32(args, "timeout_ms").unwrap_or(20_000);

        run_raw_expect_pattern(
            self.logger,
            self.console,
            &name,
            &command,
            &expected_pattern,
            Duration::from_millis(timeout_ms as u64),
        )
    }

    fn invoke_burst_batch_start(&mut self, args: &Value) -> Result<()> {
        let commands = args
            .get("commands")
            .and_then(|value| value.as_array())
            .ok_or_else(|| anyhow!("burst_batch_start requires array argument 'commands'"))?;

        if commands.is_empty() {
            return Err(anyhow!("burst_batch_start requires at least one command"));
        }

        let mark = self.console.mark();
        for command in commands {
            let command = command
                .as_str()
                .ok_or_else(|| anyhow!("burst_batch_start commands must be strings"))?;
            self.console.send_line(&self.resolve(command)?)?;
        }

        self.burst_mark = Some(mark);
        Ok(())
    }

    fn invoke_burst_batch_assert(&mut self, args: &Value) -> Result<()> {
        let name = args
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("burst_sequence")
            .to_string();
        let expected_sdreq_count = optional_arg_u32(args, "expected_sdreq_count").unwrap_or(1);
        let expected_status = args
            .get("expected_status")
            .and_then(|value| value.as_str())
            .unwrap_or("ok");
        let expected_code = args.get("expected_code").and_then(|value| value.as_str());
        let poll_timeout_ms = optional_arg_u32(args, "poll_timeout_ms").unwrap_or(30_000);
        let sdwait_timeout_ms =
            optional_arg_u32(args, "sdwait_timeout_ms").unwrap_or(self.sdwait_timeout_ms);

        let busy_pattern = args
            .get("busy_pattern")
            .and_then(|value| value.as_str())
            .unwrap_or(r"SDFAT(MKDIR|WRITE|APPEND|STAT|READ) BUSY");
        let busy_re = Regex::new(&self.resolve(busy_pattern)?)?;

        let start = self
            .burst_mark
            .ok_or_else(|| anyhow!("burst_batch_assert called before burst_batch_start"))?;

        let sdreq_re = Regex::new(r"^SDREQ id=[0-9]+ op=")?;
        let deadline = Instant::now() + Duration::from_millis(poll_timeout_ms as u64);
        while Instant::now() < deadline {
            self.console.poll_once()?;
            if self.console.count_regex_since(start, &sdreq_re) >= expected_sdreq_count as usize {
                break;
            }
            thread::sleep(Duration::from_millis(150));
        }

        if self.console.count_regex_since(start, &sdreq_re) < expected_sdreq_count as usize {
            return Err(anyhow!(
                "{name}: observed fewer than {expected_sdreq_count} SDREQ lines"
            ));
        }

        let last_line = self
            .console
            .last_regex_since(start, &sdreq_re)
            .ok_or_else(|| anyhow!("{name}: missing SDREQ lines"))?;
        let id_caps = Regex::new(r"id=([0-9]+)")?
            .captures(&last_line)
            .ok_or_else(|| anyhow!("{name}: failed parsing last SDREQ id"))?;
        let req_id = id_caps
            .get(1)
            .ok_or_else(|| anyhow!("{name}: missing SDREQ capture"))?
            .as_str()
            .parse::<u32>()?;
        wait_for_sd_result(
            self.console,
            req_id,
            sdwait_timeout_ms,
            expected_status,
            expected_code,
        )?;

        if self.console.has_regex_since(start, &busy_re) {
            return Err(anyhow!("{name}: burst flow emitted BUSY markers"));
        }

        self.logger.info(format!("[PASS] {name}"));
        self.burst_mark = None;
        Ok(())
    }
}

impl WorkflowRuntime for SdcardScenarioRuntime<'_> {
    fn invoke(&mut self, action: &str, args: &Value, _context: &mut Value) -> Result<()> {
        match action {
            "run_step" => self.invoke_run_step(args),
            "raw_expect_pattern" => self.invoke_raw_expect_pattern(args),
            "burst_batch_start" => self.invoke_burst_batch_start(args),
            "burst_batch_assert" => self.invoke_burst_batch_assert(args),
            "complete" => Ok(()),
            other => Err(anyhow!("unsupported sdcard workflow action: {other}")),
        }
    }
}

pub fn run_sdcard_hw(logger: &mut Logger, opts: SdcardHwOptions) -> Result<()> {
    maybe_flash_first(logger, &opts.build_mode)?;

    let verify_lba = env_utils::parse_env_u32("HOSTCTL_SDCARD_VERIFY_LBA", 2048)?;
    let run_tag = Local::now().format("%H%M%S").to_string();
    let base_path =
        std::env::var("HOSTCTL_SDCARD_BASE_PATH").unwrap_or_else(|_| format!("/sd{run_tag}"));
    let sdwait_timeout_ms = env_utils::parse_env_u32("HOSTCTL_SDCARD_SDWAIT_TIMEOUT_MS", 300_000)?;

    let output_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(format!(
            "logs/sdcard_hw_test_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ))
    });

    logger.info(format!(
        "Starting serial capture: {}",
        output_path.display()
    ));
    let mut console = open_console(&output_path)?;

    logger.info(format!(
        "Running SD-card command validation on {}",
        env_utils::require_port()?
    ));
    logger.info(format!("Test root path: {base_path}"));
    force_upload_mode_off(logger, &mut console)?;

    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/sdcard-hw.sw.yaml"),
    )?;

    let test_file = format!("{base_path}/io.txt");
    let test_file_renamed = format!("{base_path}/io2.txt");
    let burst_root = format!("/b{run_tag}");
    let burst_file = format!("{burst_root}/io.txt");
    let fail_root = format!("/f{run_tag}");
    let rename_root = format!("/r{run_tag}");
    let file_a = format!("{rename_root}/a.txt");
    let file_b = format!("{rename_root}/b.txt");
    let long_payload = "x".repeat(260);

    let vars = HashMap::from([
        ("run_tag".to_string(), run_tag),
        ("verify_lba".to_string(), verify_lba.to_string()),
        ("base_path".to_string(), base_path.clone()),
        ("test_file".to_string(), test_file),
        ("test_file_renamed".to_string(), test_file_renamed),
        ("burst_root".to_string(), burst_root),
        ("burst_file".to_string(), burst_file),
        ("fail_root".to_string(), fail_root),
        ("rename_root".to_string(), rename_root),
        ("file_a".to_string(), file_a),
        ("file_b".to_string(), file_b),
        ("long_payload".to_string(), long_payload),
    ]);

    let mut runtime = SdcardScenarioRuntime {
        logger,
        console: &mut console,
        vars,
        sdwait_timeout_ms,
        burst_mark: None,
    };
    let _ = execute_workflow(
        &workflow,
        &mut runtime,
        &json!({
            "suite": suite_name(&opts.suite),
        }),
    )?;

    logger.info("SD-card hardware test passed");
    logger.info(format!("Log: {}", output_path.display()));

    Ok(())
}

pub fn run_sdcard_burst_regression(
    logger: &mut Logger,
    build_mode: String,
    output_path: Option<PathBuf>,
) -> Result<()> {
    run_sdcard_hw(
        logger,
        SdcardHwOptions {
            build_mode,
            output_path,
            suite: SdcardSuite::Burst,
        },
    )
}
