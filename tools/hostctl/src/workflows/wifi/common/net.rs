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
    if matches!(expected_ack_op(command), Some("start" | "stop")) {
        return wait_net_state_ack(console, command, Duration::from_secs(155));
    }
    let expected_op = expected_ack_op(command);
    let display_command = redact_net_command(command);
    let mut op_mismatch_count = 0u32;
    let mut last_mismatch_line: Option<String> = None;
    for _ in 0..12 {
        let (status, line) = console.command_wait_ack(command, "NET", Duration::from_secs(4))?;
        match status {
            AckStatus::Ok => {
                if let Some(op) = expected_op {
                    if !ack_line_matches_op(line.as_deref(), op) {
                        op_mismatch_count = op_mismatch_count.saturating_add(1);
                        last_mismatch_line = if expected_op == Some("config_set") {
                            Some("configuration acknowledgement redacted".into())
                        } else {
                            line
                        };
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
                let detail = if expected_op == Some("config_set") {
                    "NETCFG SET [redacted]: NET ERR".to_string()
                } else {
                    line.unwrap_or_else(|| "NET ERR".to_string())
                };
                return Err(anyhow!("{detail}"));
            }
        }
    }
    if op_mismatch_count > 0 {
        return Err(anyhow!(
            "{display_command}: NET OK ack op mismatch count={} last_line={}",
            op_mismatch_count,
            last_mismatch_line.unwrap_or_else(|| "<none>".to_string())
        ));
    }
    Err(anyhow!("{display_command}: no NET OK ack"))
}

fn wait_net_state_ack(console: &mut SerialConsole, command: &str, timeout: Duration) -> Result<()> {
    // A timed-out state command may still be applying display/persistence work.
    // Resending it would create uncorrelated acknowledgements for later commands.
    let (status, line) = console.command_wait_ack(command, "NET", timeout)?;
    match (status, line.as_deref()) {
        (AckStatus::Ok, Some(line))
            if expected_ack_op(command).is_some_and(|op| ack_line_matches_op(Some(line), op)) =>
        {
            Ok(())
        }
        (AckStatus::None, _) => Err(anyhow!(
            "{command}: no NET OK ack; outcome unknown, command was not retried"
        )),
        (_, Some(line)) => Err(anyhow!("{command}: {line}")),
        _ => Err(anyhow!("{command}: no unambiguous NET acknowledgement")),
    }
}

/// Configuration payloads contain credentials and must never enter diagnostics.
fn redact_net_command(command: &str) -> &str {
    if command.trim_start().starts_with("NETCFG SET") {
        "NETCFG SET [redacted]"
    } else {
        command
    }
}

pub fn apply_state_command(console: &mut SerialConsole, args: &serde_json::Value) -> Result<()> {
    let command = args["command"]
        .as_str()
        .ok_or_else(|| anyhow!("state command is required"))?;
    let (status, line) = console.command_wait_ack(command, "STATE", Duration::from_secs(4))?;
    if status != AckStatus::Ok {
        return Err(anyhow!(
            "state command failed: {command} ({})",
            line.as_deref().unwrap_or("no STATE acknowledgement")
        ));
    }
    Ok(())
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
    use std::{
        io::{Read, Write},
        sync::mpsc,
        thread,
        time::Duration,
    };

    use anyhow::{anyhow, Result};
    use serialport::{SerialPort, TTYPort};

    use super::{
        ack_line_matches_op, apply_state_command, expected_ack_op, wait_net_ack,
        wait_net_state_ack, SerialConsole,
    };

    #[test]
    fn stop_waits_for_slow_application_ack_without_resending() -> Result<()> {
        let (mut master, mut slave) = TTYPort::pair()?;
        master.set_timeout(Duration::from_secs(1))?;
        slave.set_timeout(Duration::from_millis(5))?;
        let (done, keep_open) = mpsc::channel();
        let responder = thread::spawn(move || -> Result<()> {
            let mut command = [0; 10];
            master.read_exact(&mut command)?;
            assert_eq!(&command, b"NET STOP\r\n");
            thread::sleep(Duration::from_millis(4_100));
            master.write_all(b"NET OK op=stop\r\n")?;
            master.flush()?;
            keep_open.recv_timeout(Duration::from_secs(5))?;
            let mut extra = [0; 1];
            assert!(master.read(&mut extra).is_err(), "state command was resent");
            Ok(())
        });
        let mut console = SerialConsole::from_port_for_tests(Box::new(slave), None)?;
        let result = wait_net_ack(&mut console, "NET STOP");
        done.send(())?;
        responder
            .join()
            .map_err(|_| anyhow!("stop responder panicked"))??;
        result
    }

    #[test]
    fn state_ack_timeout_is_ambiguous_and_does_not_resend() -> Result<()> {
        let (mut master, mut slave) = TTYPort::pair()?;
        master.set_timeout(Duration::from_millis(50))?;
        slave.set_timeout(Duration::from_millis(5))?;
        let (done, keep_open) = mpsc::channel();
        let responder = thread::spawn(move || -> Result<()> {
            let mut command = [0; 11];
            master.read_exact(&mut command)?;
            assert_eq!(&command, b"NET START\r\n");
            keep_open.recv_timeout(Duration::from_secs(5))?;
            let mut extra = [0; 1];
            assert!(master.read(&mut extra).is_err(), "state command was resent");
            Ok(())
        });
        let mut console = SerialConsole::from_port_for_tests(Box::new(slave), None)?;
        let error = wait_net_state_ack(&mut console, "NET START", Duration::from_millis(30))
            .expect_err("missing application acknowledgement must fail");
        assert!(error.to_string().contains("outcome unknown"));
        done.send(())?;
        responder
            .join()
            .map_err(|_| anyhow!("state timeout responder panicked"))??;
        Ok(())
    }

    fn apply_state_with_reply(reply: &'static str) -> Result<Result<()>> {
        const COMMAND: &str = "STATE SET upload=on";
        let (mut master, mut slave) = TTYPort::pair()?;
        master.set_timeout(Duration::from_secs(1))?;
        slave.set_timeout(Duration::from_millis(5))?;
        let (done, keep_open) = mpsc::channel();
        let responder = thread::spawn(move || -> Result<Vec<u8>> {
            let mut received = vec![0; COMMAND.len() + 2];
            master.read_exact(&mut received)?;
            master.write_all(reply.as_bytes())?;
            master.flush()?;
            // Closing the PTY before the host consumes the reply can discard it.
            keep_open.recv_timeout(Duration::from_secs(5))?;
            Ok(received)
        });
        let mut console = SerialConsole::from_port_for_tests(Box::new(slave), None)?;
        let result = apply_state_command(&mut console, &serde_json::json!({ "command": COMMAND }));
        done.send(())?;
        let received = responder
            .join()
            .map_err(|_| anyhow!("state responder panicked"))??;
        assert_eq!(received, format!("{COMMAND}\r\n").as_bytes());
        Ok(result)
    }

    #[test]
    fn apply_state_command_accepts_state_ok() -> Result<()> {
        apply_state_with_reply("STATE OK\r\n")??;
        Ok(())
    }

    #[test]
    fn apply_state_command_rejects_state_busy() -> Result<()> {
        let error = apply_state_with_reply("NET OK op=start\r\nSTATE BUSY\r\n")?.unwrap_err();
        assert_eq!(
            error.to_string(),
            "state command failed: STATE SET upload=on (STATE BUSY)"
        );
        Ok(())
    }

    #[test]
    fn apply_state_command_rejects_state_err() -> Result<()> {
        let error = apply_state_with_reply("STATE ERR reason=invalid\r\n")?.unwrap_err();
        assert_eq!(
            error.to_string(),
            "state command failed: STATE SET upload=on (STATE ERR reason=invalid)"
        );
        Ok(())
    }

    #[test]
    fn configuration_error_command_redacts_credentials() {
        let command = r#"  NETCFG SET {"ssid":"private","password":"do-not-log"}"#;
        let diagnostic = format!("{}: no NET OK ack", super::redact_net_command(command));
        assert_eq!(diagnostic, "NETCFG SET [redacted]: no NET OK ack");
        assert!(!diagnostic.contains("private"));
        assert!(!diagnostic.contains("do-not-log"));
        assert_eq!(super::redact_net_command("NET START"), "NET START");
    }

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
