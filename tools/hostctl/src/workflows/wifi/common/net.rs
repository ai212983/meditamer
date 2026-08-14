use std::{sync::OnceLock, thread, time::Duration};

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
    let expected_op = expected_ack_op(command);
    let mut op_mismatch_count = 0u32;
    let mut last_mismatch_line: Option<String> = None;
    for _ in 0..12 {
        let (status, line) = console.command_wait_ack(command, "NET", Duration::from_secs(4))?;
        match status {
            AckStatus::Ok => {
                if let Some(op) = expected_op {
                    if !ack_line_matches_op(line.as_deref(), op) {
                        op_mismatch_count = op_mismatch_count.saturating_add(1);
                        last_mismatch_line = line;
                        thread::sleep(Duration::from_millis(250));
                        continue;
                    }
                }
                return Ok(());
            }
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
    if op_mismatch_count > 0 {
        return Err(anyhow!(
            "{command}: NET OK ack op mismatch count={} last_line={}",
            op_mismatch_count,
            last_mismatch_line.unwrap_or_else(|| "<none>".to_string())
        ));
    }
    Err(anyhow!("{command}: no NET OK ack"))
}

fn expected_ack_op(command: &str) -> Option<&'static str> {
    let trimmed = command.trim_start();
    if trimmed.starts_with("NET LISTENER ON") {
        return Some("listener_on");
    }
    if trimmed.starts_with("NET LISTENER OFF") {
        return Some("listener_off");
    }
    if trimmed.starts_with("NET START") {
        return Some("start");
    }
    if trimmed.starts_with("NET STOP") {
        return Some("stop");
    }
    if trimmed.starts_with("NET RECOVER") {
        return Some("recover");
    }
    if trimmed.starts_with("NETCFG SET") {
        return Some("config_set");
    }
    None
}

fn ack_line_matches_op(line: Option<&str>, expected_op: &str) -> bool {
    let Some(line) = line else {
        return false;
    };
    if !line.contains(" OK") {
        return false;
    }
    if !line.contains("op=") {
        // Backward-compatible path for firmware that only prints `NET OK`.
        return true;
    }
    let Some(op_start) = line.find("op=") else {
        return false;
    };
    let op_value = &line[op_start + 3..];
    let token_len = op_value.find([' ', '\t', '\r']).unwrap_or(op_value.len());
    &op_value[..token_len] == expected_op
}

pub fn parse_net_status_line(line: &str) -> Result<NetStatus> {
    let payload = line
        .strip_prefix("NET_STATUS ")
        .ok_or_else(|| anyhow!("invalid NET_STATUS line: {line}"))?;
    serde_json::from_str::<NetStatus>(payload).context("invalid NET_STATUS json payload")
}

pub fn query_net_status_line(console: &mut SerialConsole) -> Result<Option<String>> {
    let status_re = net_status_line_re();
    let mark = console.mark();
    console.send_line("NET STATUS")?;
    console.wait_for_regex_since(mark, status_re, Duration::from_secs(2))
}

pub fn net_status_line_re() -> &'static Regex {
    static STATUS_RE: OnceLock<Regex> = OnceLock::new();
    STATUS_RE
        .get_or_init(|| Regex::new(r"^NET_STATUS \{").expect("failed to compile NET_STATUS regex"))
}

pub fn query_net_status(console: &mut SerialConsole) -> Result<Option<NetStatus>> {
    let Some(line) = query_net_status_line(console)? else {
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
        status.listener.unwrap_or(false) && status.listener_enabled.unwrap_or(true)
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

#[cfg(test)]
mod tests {
    use super::{ack_line_matches_op, expected_ack_op};

    #[test]
    fn expected_ack_op_maps_known_commands() {
        assert_eq!(expected_ack_op("NET LISTENER ON"), Some("listener_on"));
        assert_eq!(expected_ack_op("NET LISTENER OFF"), Some("listener_off"));
        assert_eq!(expected_ack_op("NET START"), Some("start"));
        assert_eq!(expected_ack_op("NET STOP"), Some("stop"));
        assert_eq!(expected_ack_op("NET RECOVER"), Some("recover"));
        assert_eq!(
            expected_ack_op("NETCFG SET {\"ssid\":\"x\"}"),
            Some("config_set")
        );
        assert_eq!(expected_ack_op("NET STATUS"), None);
    }

    #[test]
    fn ack_line_matches_expected_op_and_allows_legacy_plain_ok() {
        assert!(ack_line_matches_op(
            Some("NET OK op=listener_on"),
            "listener_on"
        ));
        assert!(!ack_line_matches_op(Some("NET OK op=start"), "listener_on"));
        assert!(ack_line_matches_op(Some("NET OK"), "listener_on"));
        assert!(!ack_line_matches_op(None, "listener_on"));
    }
}
