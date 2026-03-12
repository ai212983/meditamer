use std::{path::Path, process::Command, thread, time::Duration};

use anyhow::{anyhow, Context, Result};
use regex::Regex;

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    serial_console::{AckStatus, SerialConsole},
    workflows::common::repo_root,
};

pub(super) fn open_console(output_path: &Path) -> Result<SerialConsole> {
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115200)?;
    ensure_parent_dir(output_path)?;
    SerialConsole::open(&port, baud, Some(output_path))
}

pub(super) fn maybe_flash_first(logger: &mut Logger, build_mode: &str) -> Result<()> {
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

pub(super) fn wait_for_sd_result(
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

pub(super) fn force_upload_mode_off(
    logger: &mut Logger,
    console: &mut SerialConsole,
) -> Result<()> {
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
pub(super) fn run_step(
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

pub(super) fn run_raw_expect_pattern(
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
