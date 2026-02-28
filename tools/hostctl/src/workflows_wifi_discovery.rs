use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::{AckStatus, SerialConsole},
};

#[derive(Clone, Debug)]
pub struct WifiDiscoveryDebugOptions {
    pub output_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct NetPolicy {
    connect_timeout_ms: u32,
    dhcp_timeout_ms: u32,
    pinned_dhcp_timeout_ms: u32,
    listener_timeout_ms: u32,
    scan_active_min_ms: u32,
    scan_active_max_ms: u32,
    scan_passive_ms: u32,
    retry_same_max: u8,
    rotate_candidate_max: u8,
    rotate_auth_max: u8,
    full_scan_reset_max: u8,
    driver_restart_max: u8,
    cooldown_ms: u32,
    driver_restart_backoff_ms: u32,
}

#[derive(Clone, Debug, Deserialize)]
struct NetStatus {
    state: Option<String>,
    link: Option<bool>,
    ipv4: Option<String>,
    listener: Option<bool>,
    failure_class: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default)]
struct DiscoveryProfile {
    rounds: u32,
    round_timeout_ms: u32,
    poll_interval_ms: u32,
    status_poll_ms: u32,
    recover_before_round: bool,
    recover_after_round: bool,
    recover_settle_ms: u32,
    max_zero_discovery_rounds: u32,
    min_ready_rounds: u32,
    min_ssid_seen_rounds: u32,
}

impl Default for DiscoveryProfile {
    fn default() -> Self {
        Self {
            rounds: 8,
            // Baseline; runtime computes an effective timeout from firmware policy
            // budgets to avoid host-side premature failure classification.
            round_timeout_ms: 60_000,
            // Poll UART transcript at 4Hz for responsive diagnostics with low CPU load.
            poll_interval_ms: 250,
            // Poll NET STATUS at 1Hz to track state transitions without command spam.
            status_poll_ms: 1_000,
            recover_before_round: true,
            recover_after_round: false,
            // Allow firmware NET RECOVER path to settle before judging readiness.
            recover_settle_ms: 1_200,
            max_zero_discovery_rounds: 0,
            min_ready_rounds: 1,
            min_ssid_seen_rounds: 1,
        }
    }
}

#[derive(Clone, Debug)]
struct RoundSample {
    round: u32,
    ready: bool,
    zero_discovery: bool,
    scan_zero_events: u32,
    scan_nonzero_events: u32,
    no_ap_found_events: u32,
    ssid_seen_events: u32,
    failure_class: String,
}

struct WifiDiscoveryRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    ssid: String,
    password: String,
    policy: NetPolicy,
    profile: DiscoveryProfile,
    samples: Vec<RoundSample>,
    ready_rounds: u32,
    zero_discovery_rounds: u32,
    ssid_seen_rounds: u32,
    total_scan_zero_events: u32,
    total_scan_nonzero_events: u32,
    total_no_ap_found_events: u32,
}

pub fn run_wifi_discovery_debug(
    logger: &mut Logger,
    opts: WifiDiscoveryDebugOptions,
) -> Result<()> {
    let port = std::env::var("HOSTCTL_NET_PORT")
        .context("HOSTCTL_NET_PORT must be set (wifi discovery debug)")?;
    let baud = std::env::var("HOSTCTL_NET_BAUD")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(115200);
    let ssid = std::env::var("HOSTCTL_NET_SSID")
        .context("HOSTCTL_NET_SSID must be set (wifi discovery debug)")?;
    let password = std::env::var("HOSTCTL_NET_PASSWORD").unwrap_or_default();
    let policy_path = std::env::var("HOSTCTL_NET_POLICY_PATH")
        .context("HOSTCTL_NET_POLICY_PATH must be set (wifi discovery debug)")?;
    let profile_path = std::env::var("HOSTCTL_NET_DISCOVERY_PROFILE_PATH").unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scenarios/wifi-discovery-debug.default.toml")
            .display()
            .to_string()
    });

    let log_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(
            std::env::var("HOSTCTL_NET_LOG_PATH").unwrap_or_else(|_| {
                format!(
                    "logs/wifi_discovery_debug_{}.log",
                    chrono::Local::now().format("%Y%m%d_%H%M%S")
                )
            }),
        )
    });
    ensure_parent_dir(&log_path)?;

    let mut console = SerialConsole::open(&port, baud, Some(&log_path))?;
    preflight(&mut console)?;

    let policy_raw = fs::read_to_string(&policy_path)
        .with_context(|| format!("failed reading HOSTCTL_NET_POLICY_PATH: {policy_path}"))?;
    let policy = serde_json::from_str::<NetPolicy>(&policy_raw)
        .context("invalid HOSTCTL_NET_POLICY_PATH JSON")?;

    let profile_raw = fs::read_to_string(&profile_path)
        .with_context(|| format!("failed reading HOSTCTL_NET_DISCOVERY_PROFILE_PATH: {profile_path}"))?;
    let profile =
        toml::from_str::<DiscoveryProfile>(&profile_raw).context("invalid TOML discovery profile")?;

    if profile.rounds == 0 {
        return Err(anyhow!("discovery profile must set rounds >= 1"));
    }

    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-discovery-debug.sw.yaml"),
    )?;

    let mut runtime = WifiDiscoveryRuntime {
        logger,
        console,
        ssid,
        password,
        policy,
        profile,
        samples: Vec::new(),
        ready_rounds: 0,
        zero_discovery_rounds: 0,
        ssid_seen_rounds: 0,
        total_scan_zero_events: 0,
        total_scan_nonzero_events: 0,
        total_no_ap_found_events: 0,
    };
    execute_workflow(&workflow, &mut runtime, &json!({}))?;
    Ok(())
}

fn preflight(console: &mut SerialConsole) -> Result<()> {
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

fn wait_net_ack(console: &mut SerialConsole, command: &str) -> Result<()> {
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

fn parse_net_status_line(line: &str) -> Result<NetStatus> {
    let payload = line
        .strip_prefix("NET_STATUS ")
        .ok_or_else(|| anyhow!("invalid NET_STATUS line: {line}"))?;
    serde_json::from_str::<NetStatus>(payload).context("invalid NET_STATUS json payload")
}

fn is_ready(status: &NetStatus) -> bool {
    matches!(status.state.as_deref(), Some("Ready"))
        && status.link.unwrap_or(false)
        && status.listener.unwrap_or(false)
        && status
            .ipv4
            .as_deref()
            .is_some_and(|ipv4| ipv4 != "0.0.0.0")
}

fn parse_scan_done_count(line: &str) -> Option<u32> {
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

fn active_scan_timeout_ms(policy: &NetPolicy) -> u64 {
    // Mirror firmware timeout shaping so host and firmware evaluate the same
    // scan-budget window.
    // Source (per-channel dwell semantics): https://docs.espressif.com/projects/rust/esp-radio/0.16.0/esp32s3/src/esp_radio/wifi/mod.rs.html
    (policy.scan_active_max_ms.max(policy.scan_active_min_ms) as u64)
        .saturating_mul(10)
        .clamp(8_000, 25_000)
}

fn passive_scan_timeout_ms(policy: &NetPolicy) -> u64 {
    // Mirror firmware timeout shaping; otherwise host may label in-progress
    // passive scans as "zero discovery" too early.
    // Source (per-channel passive dwell + 1500ms caution): https://docs.espressif.com/projects/rust/esp-radio/0.16.0/esp32s3/src/esp_radio/wifi/mod.rs.html
    (policy.scan_passive_ms as u64)
        .saturating_mul(16)
        .saturating_add(3_000)
        .clamp(15_000, 90_000)
}

fn recommended_round_timeout_ms(policy: &NetPolicy, profile: &DiscoveryProfile) -> u64 {
    // Keep host round timeout aligned with firmware discovery/recovery budgets.
    // If this is shorter than firmware's scan/watchdog windows, host will
    // misclassify healthy in-progress recovery as "zero discovery".
    let scan_budget_ms = active_scan_timeout_ms(policy)
        .saturating_add(passive_scan_timeout_ms(policy))
        .saturating_add(6_000);
    let watchdog_timeout_ms = (policy.connect_timeout_ms as u64)
        .saturating_add(scan_budget_ms)
        .max((policy.connect_timeout_ms as u64).saturating_mul(2));
    let recover_budget_ms = policy.driver_restart_backoff_ms as u64 + profile.recover_settle_ms as u64;
    let recommended = watchdog_timeout_ms
        .saturating_add(recover_budget_ms)
        .saturating_add(5_000);
    recommended.max(profile.round_timeout_ms as u64)
}

impl WorkflowRuntime for WifiDiscoveryRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "start_run" => {
                ctx_set_u32(context, "round", 1)?;
                ctx_set_u32(context, "rounds", self.profile.rounds)?;
                ctx_set_bool(context, "run_passed", false)?;
                ctx_set_string(context, "run_error", "")?;
                self.logger.info(format!(
                    "wifi-discovery-debug: effective_round_timeout_ms={} (profile_round_timeout_ms={})",
                    recommended_round_timeout_ms(&self.policy, &self.profile),
                    self.profile.round_timeout_ms
                ));
                Ok(())
            }
            "net_apply_config" => {
                let payload = json!({
                    "ssid": self.ssid,
                    "password": self.password,
                    "connect_timeout_ms": self.policy.connect_timeout_ms,
                    "dhcp_timeout_ms": self.policy.dhcp_timeout_ms,
                    "pinned_dhcp_timeout_ms": self.policy.pinned_dhcp_timeout_ms,
                    "listener_timeout_ms": self.policy.listener_timeout_ms,
                    "scan_active_min_ms": self.policy.scan_active_min_ms,
                    "scan_active_max_ms": self.policy.scan_active_max_ms,
                    "scan_passive_ms": self.policy.scan_passive_ms,
                    "retry_same_max": self.policy.retry_same_max,
                    "rotate_candidate_max": self.policy.rotate_candidate_max,
                    "rotate_auth_max": self.policy.rotate_auth_max,
                    "full_scan_reset_max": self.policy.full_scan_reset_max,
                    "driver_restart_max": self.policy.driver_restart_max,
                    "cooldown_ms": self.policy.cooldown_ms,
                    "driver_restart_backoff_ms": self.policy.driver_restart_backoff_ms,
                })
                .to_string();
                wait_net_ack(&mut self.console, &format!("NETCFG SET {payload}"))
            }
            "probe_round" => {
                let round = ctx_get_u32(context, "round")?;
                if self.profile.recover_before_round {
                    if let Err(err) = wait_net_ack(&mut self.console, "NET RECOVER") {
                        self.logger
                            .info(format!("round {round}: NET RECOVER before probe failed ({err})"));
                    }
                    self.console
                        .settle(self.profile.recover_settle_ms as u64)
                        .ok();
                }

                let mark = self.console.mark();
                if let Err(err) = wait_net_ack(&mut self.console, "NET START") {
                    self.logger
                        .info(format!("round {round}: NET START ack not obtained ({err})"));
                }

                let mut ready = false;
                let mut scan_zero_events = 0u32;
                let mut scan_nonzero_events = 0u32;
                let mut no_ap_found_events = 0u32;
                let mut ssid_seen_events = 0u32;
                let mut last_status: Option<NetStatus> = None;
                let mut read_mark = mark;
                let mut next_status_poll = Instant::now();
                let deadline = Instant::now()
                    + Duration::from_millis(recommended_round_timeout_ms(&self.policy, &self.profile));
                while Instant::now() < deadline {
                    self.console.poll_once()?;
                    for line in self.console.read_recent_lines(read_mark) {
                        read_mark = read_mark.saturating_add(1);
                        if let Some(count) = parse_scan_done_count(&line) {
                            if count == 0 {
                                scan_zero_events = scan_zero_events.saturating_add(1);
                            } else {
                                scan_nonzero_events = scan_nonzero_events.saturating_add(1);
                            }
                        }
                        if line.contains("reason=201") || line.contains("no_ap_found") {
                            no_ap_found_events = no_ap_found_events.saturating_add(1);
                        }
                        if line.starts_with("upload_http: scan ap ssid=")
                            && line.contains(&format!("scan ap ssid={}", self.ssid))
                        {
                            ssid_seen_events = ssid_seen_events.saturating_add(1);
                        }
                        if line.starts_with("NET_STATUS ") {
                            if let Ok(status) = parse_net_status_line(&line) {
                                if is_ready(&status) {
                                    ready = true;
                                }
                                last_status = Some(status);
                            }
                        }
                    }
                    if ready {
                        break;
                    }
                    if Instant::now() >= next_status_poll {
                        let _ = self.console.send_line("NET STATUS");
                        next_status_poll = Instant::now()
                            + Duration::from_millis(self.profile.status_poll_ms as u64);
                    }
                    thread::sleep(Duration::from_millis(self.profile.poll_interval_ms as u64));
                }

                let zero_discovery = !ready && scan_zero_events > 0 && scan_nonzero_events == 0;
                if ready {
                    self.ready_rounds = self.ready_rounds.saturating_add(1);
                }
                if zero_discovery {
                    self.zero_discovery_rounds = self.zero_discovery_rounds.saturating_add(1);
                }
                if ssid_seen_events > 0 {
                    self.ssid_seen_rounds = self.ssid_seen_rounds.saturating_add(1);
                }
                self.total_scan_zero_events = self
                    .total_scan_zero_events
                    .saturating_add(scan_zero_events);
                self.total_scan_nonzero_events = self
                    .total_scan_nonzero_events
                    .saturating_add(scan_nonzero_events);
                self.total_no_ap_found_events = self
                    .total_no_ap_found_events
                    .saturating_add(no_ap_found_events);

                let failure_class = last_status
                    .as_ref()
                    .and_then(|status| status.failure_class.clone())
                    .unwrap_or_else(|| "none".to_string());

                self.samples.push(RoundSample {
                    round,
                    ready,
                    zero_discovery,
                    scan_zero_events,
                    scan_nonzero_events,
                    no_ap_found_events,
                    ssid_seen_events,
                    failure_class: failure_class.clone(),
                });

                self.logger.info(format!(
                    "round {}: ready={} zero_discovery={} scan_zero={} scan_nonzero={} no_ap_found={} ssid_seen={} failure_class={}",
                    round,
                    ready,
                    zero_discovery,
                    scan_zero_events,
                    scan_nonzero_events,
                    no_ap_found_events,
                    ssid_seen_events,
                    failure_class
                ));

                if self.profile.recover_after_round {
                    if let Err(err) = wait_net_ack(&mut self.console, "NET RECOVER") {
                        self.logger
                            .info(format!("round {round}: NET RECOVER after probe failed ({err})"));
                    }
                    self.console
                        .settle(self.profile.recover_settle_ms as u64)
                        .ok();
                }

                ctx_set_bool(context, "round_ready", ready)?;
                ctx_set_bool(context, "round_zero_discovery", zero_discovery)?;
                Ok(())
            }
            "advance_round" => {
                let round = ctx_get_u32(context, "round")?;
                ctx_set_u32(context, "round", round.saturating_add(1))
            }
            "evaluate_results" => {
                let pass_zero = self.zero_discovery_rounds <= self.profile.max_zero_discovery_rounds;
                let pass_ready = self.ready_rounds >= self.profile.min_ready_rounds;
                let observed_scan_activity =
                    self.total_scan_zero_events > 0 || self.total_scan_nonzero_events > 0;
                // If rounds already reach Ready without new scan telemetry, do not
                // force ssid_seen evidence. That pattern happens when link/listener
                // are already healthy and no discovery cycle is needed.
                // Invariant: "ready" is authoritative for this diagnostic. SSID
                // evidence is only required when a discovery cycle actually ran.
                let require_ssid_evidence = observed_scan_activity || self.ready_rounds == 0;
                let pass_ssid =
                    !require_ssid_evidence || self.ssid_seen_rounds >= self.profile.min_ssid_seen_rounds;
                let passed = pass_zero && pass_ready && pass_ssid;

                let mut failures = Vec::new();
                if !pass_zero {
                    failures.push(format!(
                        "zero_discovery_rounds={} exceeds max_zero_discovery_rounds={}",
                        self.zero_discovery_rounds, self.profile.max_zero_discovery_rounds
                    ));
                }
                if !pass_ready {
                    failures.push(format!(
                        "ready_rounds={} below min_ready_rounds={}",
                        self.ready_rounds, self.profile.min_ready_rounds
                    ));
                }
                if !pass_ssid {
                    failures.push(format!(
                        "ssid_seen_rounds={} below min_ssid_seen_rounds={}",
                        self.ssid_seen_rounds, self.profile.min_ssid_seen_rounds
                    ));
                }
                let run_error = failures.join("; ");

                ctx_set_bool(context, "run_passed", passed)?;
                ctx_set_string(context, "run_error", &run_error)?;
                Ok(())
            }
            "print_summary" => {
                self.logger.info(format!(
                    "summary rounds={} ready_rounds={} zero_discovery_rounds={} ssid_seen_rounds={} total_scan_zero_events={} total_scan_nonzero_events={} total_no_ap_found_events={}",
                    self.samples.len(),
                    self.ready_rounds,
                    self.zero_discovery_rounds,
                    self.ssid_seen_rounds,
                    self.total_scan_zero_events,
                    self.total_scan_nonzero_events,
                    self.total_no_ap_found_events
                ));
                for sample in &self.samples {
                    self.logger.info(format!(
                        "round {} detail ready={} zero_discovery={} scan_zero={} scan_nonzero={} no_ap_found={} ssid_seen={} failure_class={}",
                        sample.round,
                        sample.ready,
                        sample.zero_discovery,
                        sample.scan_zero_events,
                        sample.scan_nonzero_events,
                        sample.no_ap_found_events,
                        sample.ssid_seen_events,
                        sample.failure_class
                    ));
                }
                Ok(())
            }
            "fail_run" => {
                let detail = ctx_get_string(context, "run_error")
                    .unwrap_or_else(|_| "wifi discovery debug failed".to_string());
                Err(anyhow!("{detail}"))
            }
            _ => Err(anyhow!("unknown workflow action: {action}")),
        }
    }
}

fn ctx_get_u32(context: &Value, key: &str) -> Result<u32> {
    context
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| anyhow!("missing context key `{key}` as u32"))
}

fn ctx_get_string(context: &Value, key: &str) -> Result<String> {
    context
        .get(key)
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .ok_or_else(|| anyhow!("missing context key `{key}` as string"))
}

fn ctx_set_u32(context: &mut Value, key: &str, value: u32) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}

fn ctx_set_bool(context: &mut Value, key: &str, value: bool) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}

fn ctx_set_string(context: &mut Value, key: &str, value: &str) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_scan_done_count;

    #[test]
    fn scan_done_parser_extracts_count() {
        assert_eq!(
            parse_scan_done_count("upload_http: event scan_done status=0 count=2 scan_id=42"),
            Some(2)
        );
        assert_eq!(
            parse_scan_done_count("upload_http: event scan_done status=0 count=0 scan_id=42"),
            Some(0)
        );
        assert_eq!(parse_scan_done_count("NET_STATUS {\"state\":\"Ready\"}"), None);
    }
}
