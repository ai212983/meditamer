use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
use regex::Regex;

use crate::{
    env_utils,
    logging::Logger,
    serial_console::{AckStatus, SerialConsole},
};

pub struct TimeSetOptions {
    pub epoch: Option<u64>,
    pub tz_offset_minutes: Option<i32>,
}

pub struct RepaintOptions {
    pub command: Option<String>,
}

pub struct TouchWizardDumpOptions {
    pub output_path: Option<PathBuf>,
}

fn open_console(
    settle_ms: u64,
    output_path: Option<PathBuf>,
) -> Result<(SerialConsole, String, u32)> {
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115200)?;
    let mut console = SerialConsole::open(&port, baud, output_path.as_deref())?;
    console.settle(settle_ms)?;
    Ok((console, port, baud))
}

pub fn run_timeset(logger: &mut Logger, opts: TimeSetOptions) -> Result<()> {
    let epoch = opts.epoch.unwrap_or_else(|| Utc::now().timestamp() as u64);
    let tz_offset = opts
        .tz_offset_minutes
        .unwrap_or_else(|| Local::now().offset().local_minus_utc() / 60);

    if !(-720..=840).contains(&tz_offset) {
        return Err(anyhow!("tz_offset_minutes must be within -720..840"));
    }

    let settle_ms = env_utils::parse_env_u64("HOSTCTL_TIMESET_SETTLE_MS", 1500)?;
    let retries = env_utils::parse_env_u32("HOSTCTL_TIMESET_RETRIES", 8)?;
    let retry_delay_ms = env_utils::parse_env_u64("HOSTCTL_TIMESET_RETRY_DELAY_MS", 700)?;
    let wait_ack = env_utils::parse_env_bool01("HOSTCTL_TIMESET_WAIT_ACK", true)?;
    let ack_timeout_ms = env_utils::parse_env_u64("HOSTCTL_TIMESET_ACK_TIMEOUT_MS", 1200)?;

    if retries == 0 {
        return Err(anyhow!("HOSTCTL_TIMESET_RETRIES must be >= 1"));
    }

    let (mut console, port, baud) = open_console(settle_ms, None)?;
    let command = format!("TIMESET {epoch} {tz_offset}");

    for attempt in 1..=retries {
        let (status, line) =
            console.command_wait_ack(&command, "TIMESET", Duration::from_millis(ack_timeout_ms))?;
        if wait_ack {
            if status == AckStatus::Ok {
                logger.info(format!(
                    "Sent ({attempt}x) with ACK: TIMESET {epoch} {tz_offset} -> {port} @ {baud}"
                ));
                return Ok(());
            }
            if status == AckStatus::Busy {
                thread::sleep(Duration::from_millis(retry_delay_ms));
                continue;
            }
            if status == AckStatus::Err {
                return Err(anyhow!(
                    "TIMESET returned ERR: {}",
                    line.unwrap_or_else(|| "<unknown>".into())
                ));
            }
        }

        if attempt < retries {
            thread::sleep(Duration::from_millis(retry_delay_ms));
        }
    }

    if wait_ack {
        return Err(anyhow!(
            "No TIMESET ACK after {retries} attempts: TIMESET {epoch} {tz_offset} -> {port} @ {baud}"
        ));
    }

    logger.info(format!(
        "Sent ({retries}x): TIMESET {epoch} {tz_offset} -> {port} @ {baud}"
    ));
    Ok(())
}

pub fn run_repaint(logger: &mut Logger, opts: RepaintOptions) -> Result<()> {
    let settle_ms = env_utils::parse_env_u64("HOSTCTL_REPAINT_SETTLE_MS", 200)?;
    let retries = env_utils::parse_env_u32("HOSTCTL_REPAINT_RETRIES", 2)?;
    let retry_delay_ms = env_utils::parse_env_u64("HOSTCTL_REPAINT_RETRY_DELAY_MS", 500)?;
    let wait_ack = env_utils::parse_env_bool01("HOSTCTL_REPAINT_WAIT_ACK", true)?;
    let ack_timeout_ms = env_utils::parse_env_u64("HOSTCTL_REPAINT_ACK_TIMEOUT_MS", 15_000)?;
    let command = opts
        .command
        .or_else(|| std::env::var("HOSTCTL_REPAINT_CMD").ok())
        .unwrap_or_else(|| "REPAINT".to_string());

    if retries == 0 {
        return Err(anyhow!("HOSTCTL_REPAINT_RETRIES must be >= 1"));
    }

    let (mut console, port, baud) = open_console(settle_ms, None)?;
    let ack_ok = format!("{} OK", command);
    let ack_busy = format!("{} BUSY", command);

    for attempt in 1..=retries {
        let mark = console.mark();
        console.send_line(&command)?;
        if wait_ack {
            let pattern = Regex::new(&format!(r"^{} (OK|BUSY|ERR.*)$", regex::escape(&command)))?;
            let line = console.wait_for_regex_since(
                mark,
                &pattern,
                Duration::from_millis(ack_timeout_ms),
            )?;
            if let Some(line) = line {
                if line.contains(&ack_ok) {
                    logger.info(format!(
                        "Sent ({attempt}x) with ACK: {command} -> {port} @ {baud}"
                    ));
                    return Ok(());
                }
                if line.contains(&ack_busy) {
                    thread::sleep(Duration::from_millis(retry_delay_ms));
                    continue;
                }
                if line.contains(" ERR") {
                    return Err(anyhow!("{command} failed: {line}"));
                }
            }
        }

        if attempt < retries {
            thread::sleep(Duration::from_millis(retry_delay_ms));
        }
    }

    if wait_ack {
        return Err(anyhow!(
            "No {command} ACK after {retries} attempts: {command} -> {port} @ {baud}"
        ));
    }

    logger.info(format!("Sent ({retries}x): {command} -> {port} @ {baud}"));
    Ok(())
}

pub fn run_marble_metrics(logger: &mut Logger) -> Result<()> {
    let settle_ms = env_utils::parse_env_u64("HOSTCTL_METRICS_SETTLE_MS", 500)?;
    let retries = env_utils::parse_env_u32("HOSTCTL_METRICS_RETRIES", 1)?;
    let retry_delay_ms = env_utils::parse_env_u64("HOSTCTL_METRICS_RETRY_DELAY_MS", 300)?;
    let timeout_ms = env_utils::parse_env_u64("HOSTCTL_METRICS_TIMEOUT_MS", 60_000)?;

    if retries == 0 {
        return Err(anyhow!("HOSTCTL_METRICS_RETRIES must be >= 1"));
    }

    let (mut console, _port, _baud) = open_console(settle_ms, None)?;
    let re = Regex::new(r"^METRICS\s+MARBLE_REDRAW_MS=(\d+)(?:\s+MAX_MS=(\d+))?$")?;

    for attempt in 1..=retries {
        let mark = console.mark();
        console.send_line("METRICS")?;
        if let Some(line) =
            console.wait_for_regex_since(mark, &re, Duration::from_millis(timeout_ms))?
        {
            logger.info(line);
            return Ok(());
        }
        if attempt < retries {
            thread::sleep(Duration::from_millis(retry_delay_ms));
        }
    }

    Err(anyhow!("No METRICS response after {retries} attempts"))
}

fn latest_complete_touch_dump_segment(text: &str) -> Option<String> {
    let end_idx = text.rfind("TOUCH_WIZARD_DUMP END")?;
    let begin_idx = text[..end_idx].rfind("TOUCH_WIZARD_DUMP BEGIN")?;
    let segment = &text[begin_idx..];
    Some(segment.to_string())
}

pub fn run_touch_wizard_dump(logger: &mut Logger, opts: TouchWizardDumpOptions) -> Result<()> {
    let settle_ms = env_utils::parse_env_u64("HOSTCTL_TOUCH_WIZARD_DUMP_SETTLE_MS", 200)?;
    let retries = env_utils::parse_env_u32("HOSTCTL_TOUCH_WIZARD_DUMP_RETRIES", 3)?;
    let timeout_ms = env_utils::parse_env_u64("HOSTCTL_TOUCH_WIZARD_DUMP_TIMEOUT_MS", 8000)?;
    if retries == 0 {
        return Err(anyhow!("HOSTCTL_TOUCH_WIZARD_DUMP_RETRIES must be >= 1"));
    }

    let default_output = PathBuf::from(format!(
        "logs/touch_wizard_dump_{}.log",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    ));
    let output_path = opts.output_path.unwrap_or(default_output);
    let end_re = Regex::new(r"TOUCH_WIZARD_DUMP END")?;
    let samples_re = Regex::new(r"samples=(\d+)")?;
    let line_re = Regex::new(
        r"^touch_wizard_swipe,\d+,\d+,\d+,[a-z_]+,[a-z_]+,[a-z_]+,[a-z_]+,\d+,\d+,\d+,\d+,\d+,\d+,\d+,\d+,\d+$",
    )?;

    for attempt in 1..=retries {
        let mut console = {
            let port = env_utils::require_port()?;
            let baud = env_utils::baud_from_env(115200)?;
            SerialConsole::open(&port, baud, Some(&output_path))?
        };
        console.settle(settle_ms)?;

        let mark = console.mark();
        console.send_line("TOUCH_WIZARD_DUMP")?;
        let got_end = console
            .wait_for_regex_since(mark, &end_re, Duration::from_millis(timeout_ms))?
            .is_some();

        if got_end {
            let tail_deadline = Instant::now() + Duration::from_millis(100);
            while Instant::now() < tail_deadline {
                console.poll_once()?;
            }
        }

        let all = console.read_recent_lines(0).join("\n");
        if let Some(segment) = latest_complete_touch_dump_segment(&all) {
            let expected = samples_re
                .captures(&segment)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<usize>().ok())
                .ok_or_else(|| anyhow!("missing samples header field"))?;

            let actual = segment
                .lines()
                .filter(|line| line_re.is_match(line.trim()))
                .count();
            if actual == expected {
                std::fs::write(&output_path, segment.as_bytes())?;
                logger.info(format!(
                    "TOUCH_WIZARD_DUMP OK: attempts={attempt} samples={actual} file={}",
                    output_path.display()
                ));
                return Ok(());
            }
        }

        thread::sleep(Duration::from_millis(150));
    }

    Err(anyhow!(
        "TOUCH_WIZARD_DUMP FAILED after {retries} attempts; captured={}",
        output_path.display()
    ))
}
