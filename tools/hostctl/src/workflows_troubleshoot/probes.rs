use std::{thread, time::Duration};

use anyhow::{anyhow, Result};
use chrono::Utc;
use regex::Regex;

use crate::serial_console::SerialConsole;

pub(super) fn run_uart_probe_sequence(
    console: &mut SerialConsole,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    run_ping_probe(console, retries, delay_ms, timeout_ms)?;
    run_state_probe(console, retries, delay_ms, timeout_ms)?;
    run_timeset_probe(console, retries, delay_ms, timeout_ms)?;
    run_psram_probe(console, retries, delay_ms, timeout_ms)?;
    Ok(())
}

fn run_ping_probe(
    console: &mut SerialConsole,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let re = Regex::new(r"^PONG$")?;
    run_regex_probe(
        console,
        "PING",
        &re,
        retries,
        delay_ms,
        timeout_ms,
        "missing PONG",
    )
}

fn run_state_probe(
    console: &mut SerialConsole,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let re = Regex::new(r"STATE phase=.* base=.* upload=(on|off) assets=(on|off)")?;
    run_regex_probe(
        console,
        "STATE GET",
        &re,
        retries,
        delay_ms,
        timeout_ms,
        "missing STATE GET response",
    )
}

fn run_psram_probe(
    console: &mut SerialConsole,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let re = Regex::new(r"^PSRAM feature_enabled=")?;
    run_regex_probe(
        console,
        "PSRAM",
        &re,
        retries,
        delay_ms,
        timeout_ms,
        "missing PSRAM response",
    )
}

fn run_timeset_probe(
    console: &mut SerialConsole,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let re = Regex::new(r"^TIMESET (OK|BUSY|ERR.*)$")?;
    let timeout = Duration::from_millis(timeout_ms.max(250));

    for _ in 0..retries {
        let mark = console.mark();
        let epoch = Utc::now().timestamp();
        console.send_line(&format!("TIMESET {epoch} 0"))?;

        if let Some(line) = console.wait_for_regex_since(mark, &re, timeout)? {
            if line.contains("TIMESET OK") {
                return Ok(());
            }
            if line.contains("TIMESET BUSY") {
                thread::sleep(Duration::from_millis(delay_ms));
                continue;
            }
            if line.contains("TIMESET ERR") {
                return Err(anyhow!("timeset probe returned ERR: {line}"));
            }
        }
        thread::sleep(Duration::from_millis(delay_ms));
    }

    Err(anyhow!("missing TIMESET OK response"))
}

fn run_regex_probe(
    console: &mut SerialConsole,
    command: &str,
    regex: &Regex,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
    missing_detail: &str,
) -> Result<()> {
    let timeout = Duration::from_millis(timeout_ms.max(250));
    for _ in 0..retries {
        let mark = console.mark();
        console.send_line(command)?;
        if console
            .wait_for_regex_since(mark, regex, timeout)?
            .is_some()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(delay_ms));
    }
    Err(anyhow!("{missing_detail}"))
}
