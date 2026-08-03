use std::{path::PathBuf, thread, time::Duration};

use anyhow::{anyhow, Result};
use regex::Regex;

use crate::{env_utils, logging::Logger, serial_console::SerialConsole};

pub struct RepaintOptions {
    pub command: Option<String>,
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
