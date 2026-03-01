use std::{thread, time::Duration};

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde_json::json;

use crate::serial_console::{AckStatus, SerialConsole};

use super::{NetPolicy, NetStatus};

pub fn preflight(console: &mut SerialConsole) -> Result<()> {
    let pong_re = Regex::new(r"^PONG$")?;
    for _ in 0..5 {
        if console
            .command_wait_regex("PING", &pong_re, Duration::from_secs(3))?
            .is_some()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(anyhow!("serial preflight failed: no PONG"))
}

pub fn wait_net_ack(console: &mut SerialConsole, command: &str) -> Result<()> {
    for _ in 0..12 {
        let (status, line) = console.command_wait_ack(command, "NET", Duration::from_secs(4))?;
        match status {
            AckStatus::Ok => return Ok(()),
            AckStatus::Busy | AckStatus::None => thread::sleep(Duration::from_millis(400)),
            AckStatus::Err => {
                if line
                    .as_deref()
                    .is_some_and(|detail| detail.contains("reason=busy"))
                {
                    thread::sleep(Duration::from_millis(400));
                    continue;
                }
                let detail = line.unwrap_or_else(|| "NET ERR".to_string());
                return Err(anyhow!("{detail}"));
            }
        }
    }
    Err(anyhow!("{command}: no NET OK ack"))
}

pub fn parse_net_status_line(line: &str) -> Result<NetStatus> {
    let payload = line
        .strip_prefix("NET_STATUS ")
        .ok_or_else(|| anyhow!("invalid NET_STATUS line: {line}"))?;
    serde_json::from_str::<NetStatus>(payload).context("invalid NET_STATUS json payload")
}

pub fn query_net_status(console: &mut SerialConsole) -> Result<Option<NetStatus>> {
    let status_re = Regex::new(r"^NET_STATUS \{")?;
    let mark = console.mark();
    console.send_line("NET STATUS")?;
    let Some(line) = console.wait_for_regex_since(mark, &status_re, Duration::from_secs(2))? else {
        return Ok(None);
    };
    let Ok(status) = parse_net_status_line(&line) else {
        return Ok(None);
    };
    Ok(Some(status))
}

pub fn parse_scan_done_count(line: &str) -> Option<u32> {
    if !line.starts_with("upload_http: event scan_done ") {
        return None;
    }
    let (_, after) = line.split_once("count=")?;
    let digits: String = after.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

pub fn is_ready(status: &NetStatus, require_listener: bool) -> bool {
    if status.state.as_deref() != Some("Ready") {
        return false;
    }
    if !status.link.unwrap_or(false) {
        return false;
    }
    let ipv4_ready = status.ipv4.as_deref().is_some_and(|ipv4| ipv4 != "0.0.0.0");
    if !ipv4_ready {
        return false;
    }
    if require_listener {
        status.listener.unwrap_or(false)
    } else {
        true
    }
}

pub fn netcfg_set_payload(ssid: &str, password: &str, policy: NetPolicy) -> String {
    json!({
        "ssid": ssid,
        "password": password,
        "connect_timeout_ms": policy.connect_timeout_ms,
        "dhcp_timeout_ms": policy.dhcp_timeout_ms,
        "pinned_dhcp_timeout_ms": policy.pinned_dhcp_timeout_ms,
        "listener_timeout_ms": policy.listener_timeout_ms,
        "scan_active_min_ms": policy.scan_active_min_ms,
        "scan_active_max_ms": policy.scan_active_max_ms,
        "scan_passive_ms": policy.scan_passive_ms,
        "retry_same_max": policy.retry_same_max,
        "rotate_candidate_max": policy.rotate_candidate_max,
        "rotate_auth_max": policy.rotate_auth_max,
        "full_scan_reset_max": policy.full_scan_reset_max,
        "driver_restart_max": policy.driver_restart_max,
        "cooldown_ms": policy.cooldown_ms,
        "driver_restart_backoff_ms": policy.driver_restart_backoff_ms,
    })
    .to_string()
}
