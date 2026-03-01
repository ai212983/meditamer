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
    disable_listener_during_probe_rounds: bool,
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
            // Discovery regressed to repeated `scan_done count=0` when settle was
            // too short after NET RECOVER. Keep this conservative to avoid
            // reintroducing zero-discovery loops under transient RF churn.
            recover_settle_ms: 6_000,
            disable_listener_during_probe_rounds: true,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemDiagKind {
    Radio,
    Upload,
}

#[derive(Clone, Debug)]
struct MemDiagSample {
    kind: MemDiagKind,
    stage: String,
    free: u64,
    internal_free: u64,
    external_free: u64,
    min_internal_free: u64,
}

#[derive(Clone, Debug, Default)]
struct MemDiagSummary {
    samples: u32,
    radio_samples: u32,
    upload_samples: u32,
    nomem_stage_samples: u32,
    min_free: Option<(u64, String)>,
    min_internal_free: Option<(u64, String)>,
    min_external_free: Option<(u64, String)>,
    min_internal_low_water: Option<(u64, String)>,
}

impl MemDiagSummary {
    fn record_line(&mut self, line: &str) {
        let Some(sample) = parse_mem_diag_line(line) else {
            return;
        };
        self.samples = self.samples.saturating_add(1);
        match sample.kind {
            MemDiagKind::Radio => self.radio_samples = self.radio_samples.saturating_add(1),
            MemDiagKind::Upload => self.upload_samples = self.upload_samples.saturating_add(1),
        }
        if sample.stage.contains("nomem") {
            self.nomem_stage_samples = self.nomem_stage_samples.saturating_add(1);
        }
        let label = match sample.kind {
            MemDiagKind::Radio => format!("radio:{}", sample.stage),
            MemDiagKind::Upload => format!("upload:{}", sample.stage),
        };
        update_min_sample(&mut self.min_free, sample.free, &label);
        update_min_sample(&mut self.min_internal_free, sample.internal_free, &label);
        update_min_sample(&mut self.min_external_free, sample.external_free, &label);
        update_min_sample(
            &mut self.min_internal_low_water,
            sample.min_internal_free,
            &label,
        );
    }
}

fn update_min_sample(slot: &mut Option<(u64, String)>, value: u64, label: &str) {
    match slot {
        Some((current, _)) if value >= *current => {}
        _ => *slot = Some((value, label.to_string())),
    }
}

fn token_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
}

fn token_u64(line: &str, key: &str) -> Option<u64> {
    token_value(line, key)?.parse::<u64>().ok()
}

fn parse_mem_diag_line(line: &str) -> Option<MemDiagSample> {
    let kind = if line.starts_with("upload_http: radio_mem ") {
        MemDiagKind::Radio
    } else if line.starts_with("upload_http: upload_mem ") {
        MemDiagKind::Upload
    } else {
        return None;
    };
    Some(MemDiagSample {
        kind,
        stage: token_value(line, "stage")?.to_string(),
        free: token_u64(line, "free")?,
        internal_free: token_u64(line, "internal_free")?,
        external_free: token_u64(line, "external_free")?,
        min_internal_free: token_u64(line, "min_internal_free")?,
    })
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
    mem_diag: MemDiagSummary,
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
        PathBuf::from(std::env::var("HOSTCTL_NET_LOG_PATH").unwrap_or_else(|_| {
            format!(
                "logs/wifi_discovery_debug_{}.log",
                chrono::Local::now().format("%Y%m%d_%H%M%S")
            )
        }))
    });
    ensure_parent_dir(&log_path)?;

    let mut console = SerialConsole::open(&port, baud, Some(&log_path))?;
    preflight(&mut console)?;

    let policy_raw = fs::read_to_string(&policy_path)
        .with_context(|| format!("failed reading HOSTCTL_NET_POLICY_PATH: {policy_path}"))?;
    let policy = serde_json::from_str::<NetPolicy>(&policy_raw)
        .context("invalid HOSTCTL_NET_POLICY_PATH JSON")?;

    let profile_raw = fs::read_to_string(&profile_path).with_context(|| {
        format!("failed reading HOSTCTL_NET_DISCOVERY_PROFILE_PATH: {profile_path}")
    })?;
    let profile = toml::from_str::<DiscoveryProfile>(&profile_raw)
        .context("invalid TOML discovery profile")?;

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
        mem_diag: MemDiagSummary::default(),
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

fn is_ready(status: &NetStatus, require_listener: bool) -> bool {
    if !matches!(status.state.as_deref(), Some("Ready")) {
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

fn fmt_min(value: &Option<(u64, String)>) -> String {
    match value {
        Some((bytes, stage)) => format!("{bytes}@{stage}"),
        None => "n/a".to_string(),
    }
}

struct ProbeRoundState {
    ready: bool,
    scan_zero_events: u32,
    scan_nonzero_events: u32,
    no_ap_found_events: u32,
    ssid_seen_events: u32,
    last_failure_class: String,
    read_mark: usize,
    next_status_poll: Instant,
    deadline: Instant,
    round_mem_diag: MemDiagSummary,
}

impl Default for ProbeRoundState {
    fn default() -> Self {
        Self {
            ready: false,
            scan_zero_events: 0,
            scan_nonzero_events: 0,
            no_ap_found_events: 0,
            ssid_seen_events: 0,
            last_failure_class: String::new(),
            read_mark: 0,
            next_status_poll: Instant::now(),
            deadline: Instant::now(),
            round_mem_diag: MemDiagSummary::default(),
        }
    }
}

impl ProbeRoundState {
    fn new(read_mark: usize, deadline: Instant) -> Self {
        Self {
            read_mark,
            next_status_poll: Instant::now(),
            deadline,
            last_failure_class: "none".to_string(),
            ..Self::default()
        }
    }

    fn ingest_line(&mut self, line: &str, ssid_marker: &str, require_listener: bool) {
        if let Some(count) = parse_scan_done_count(line) {
            if count == 0 {
                self.scan_zero_events = self.scan_zero_events.saturating_add(1);
            } else {
                self.scan_nonzero_events = self.scan_nonzero_events.saturating_add(1);
            }
        }
        if line.contains("reason=201") || line.contains("no_ap_found") {
            self.no_ap_found_events = self.no_ap_found_events.saturating_add(1);
        }
        if line.starts_with("upload_http: scan ap ssid=") && line.contains(ssid_marker) {
            self.ssid_seen_events = self.ssid_seen_events.saturating_add(1);
        }
        if line.starts_with("NET_STATUS ") {
            if let Ok(status) = parse_net_status_line(line) {
                self.last_failure_class = status
                    .failure_class
                    .as_deref()
                    .unwrap_or("none")
                    .to_string();
                if is_ready(&status, require_listener) {
                    self.ready = true;
                }
            }
        }
    }

    fn is_zero_discovery(&self) -> bool {
        !self.ready && self.scan_zero_events > 0 && self.scan_nonzero_events == 0
    }
}

fn active_scan_timeout_ms(policy: &NetPolicy) -> u64 {
    // Mirror firmware timeout shaping so host and firmware evaluate the same
    // scan-budget window.
    // Source (per-channel dwell semantics): https://docs.espressif.com/projects/rust/esp-radio/0.16.0/esp32s3/src/esp_radio/wifi/mod.rs.html
    (policy.scan_active_max_ms.max(policy.scan_active_min_ms) as u64)
        .saturating_mul(10)
        .clamp(8_000, 25_000)
}

fn directed_scan_timeout_ms(policy: &NetPolicy) -> u64 {
    active_scan_timeout_ms(policy).clamp(3_000, 8_000)
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

fn zero_discovery_probe_timeout_ms(policy: &NetPolicy) -> u64 {
    active_scan_timeout_ms(policy).clamp(2_500, 6_000)
}

fn zero_discovery_probe_budget_ms(policy: &NetPolicy) -> u64 {
    // Mirror firmware watchdog budgeting for forced full-channel probe path.
    // Firmware may escalate from [8,1,6,11] into full 2.4 GHz channel sweep
    // under zero-discovery hard-guard conditions.
    const ZERO_DISCOVERY_PROBE_CHANNELS: u64 = 13;
    zero_discovery_probe_timeout_ms(policy).saturating_mul(ZERO_DISCOVERY_PROBE_CHANNELS)
}

fn recommended_round_timeout_ms(policy: &NetPolicy, profile: &DiscoveryProfile) -> u64 {
    // Keep host round timeout aligned with firmware discovery/recovery budgets.
    // If this is shorter than firmware's scan/watchdog windows, host will
    // misclassify healthy in-progress recovery as "zero discovery".
    let scan_budget_ms = active_scan_timeout_ms(policy)
        .saturating_add(directed_scan_timeout_ms(policy))
        .saturating_add(passive_scan_timeout_ms(policy))
        .saturating_add(zero_discovery_probe_budget_ms(policy))
        .saturating_add(6_000);
    let watchdog_timeout_ms = (policy.connect_timeout_ms as u64)
        .saturating_add(scan_budget_ms)
        .max((policy.connect_timeout_ms as u64).saturating_mul(2));
    let recover_budget_ms =
        policy.driver_restart_backoff_ms as u64 + profile.recover_settle_ms as u64;
    let recommended = watchdog_timeout_ms
        .saturating_add(recover_budget_ms)
        .saturating_add(5_000);
    recommended.max(profile.round_timeout_ms as u64)
}

impl WorkflowRuntime for WifiDiscoveryRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "start_run" => self.handle_start_run(context),
            "net_apply_config" => self.handle_net_apply_config(),
            "probe_round" => self.handle_probe_round(context),
            "advance_round" => self.handle_advance_round(context),
            "evaluate_results" => self.handle_evaluate_results(context),
            "print_summary" => self.handle_print_summary(),
            "fail_run" => self.handle_fail_run(context),
            _ => Err(anyhow!("unknown workflow action: {action}")),
        }
    }
}

impl WifiDiscoveryRuntime<'_> {
    fn handle_start_run(&mut self, context: &mut Value) -> Result<()> {
        ctx_set_u32(context, "round", 1)?;
        ctx_set_u32(context, "rounds", self.profile.rounds)?;
        ctx_set_bool(context, "run_passed", false)?;
        ctx_set_string(context, "run_error", "")?;
        if self.profile.disable_listener_during_probe_rounds {
            wait_net_ack(&mut self.console, "NET LISTENER OFF")?;
        }
        self.logger.info(format!(
            "wifi-discovery-debug: effective_round_timeout_ms={} (profile_round_timeout_ms={})",
            recommended_round_timeout_ms(&self.policy, &self.profile),
            self.profile.round_timeout_ms
        ));
        Ok(())
    }

    fn handle_net_apply_config(&mut self) -> Result<()> {
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

    fn handle_probe_round(&mut self, context: &mut Value) -> Result<()> {
        let round = ctx_get_u32(context, "round")?;
        let ssid_marker = format!("scan ap ssid={}", self.ssid);
        let require_listener = !self.profile.disable_listener_during_probe_rounds;

        if let Err(err) = self.maybe_recover_before_round(round) {
            self.logger.info(format!(
                "round {round}: NET RECOVER before probe failed ({err})"
            ));
        }
        self.prepare_probe_round(round);
        if let Err(err) = wait_net_ack(&mut self.console, "NET START") {
            self.logger
                .info(format!("round {round}: NET START ack not obtained ({err})"));
        }

        let mut state = self.new_probe_round_state();
        while Instant::now() < state.deadline {
            state = self.collect_probe_round_lines(state, &ssid_marker, require_listener)?;
            if state.ready {
                break;
            }
            self.send_status_poll_if_ready(&mut state);
            thread::sleep(Duration::from_millis(self.profile.poll_interval_ms as u64));
        }

        let zero_discovery = state.is_zero_discovery();
        self.record_round_results(round, &state, zero_discovery);
        if self.profile.recover_after_round {
            if let Err(err) = self.maybe_recover_after_round(round) {
                self.logger.info(format!(
                    "round {round}: NET RECOVER after probe failed ({err})"
                ));
            }
        }
        ctx_set_bool(context, "round_ready", state.ready)?;
        ctx_set_bool(context, "round_zero_discovery", zero_discovery)?;
        Ok(())
    }

    fn maybe_recover_before_round(&mut self, _round: u32) -> Result<()> {
        if !self.profile.recover_before_round {
            return Ok(());
        }
        wait_net_ack(&mut self.console, "NET RECOVER")?;
        self.console.settle(self.profile.recover_settle_ms as u64)?;
        Ok(())
    }

    fn prepare_probe_round(&mut self, round: u32) {
        if !self.profile.disable_listener_during_probe_rounds {
            return;
        }
        if let Err(err) = wait_net_ack(&mut self.console, "NET LISTENER OFF") {
            self.logger
                .info(format!("round {round}: NET LISTENER OFF failed ({err})"));
        }
    }

    fn new_probe_round_state(&self) -> ProbeRoundState {
        ProbeRoundState::new(
            self.console.mark(),
            Instant::now()
                + Duration::from_millis(recommended_round_timeout_ms(&self.policy, &self.profile)),
        )
    }

    fn collect_probe_round_lines(
        &mut self,
        mut state: ProbeRoundState,
        ssid_marker: &str,
        require_listener: bool,
    ) -> Result<ProbeRoundState> {
        self.console.poll_once()?;
        for line in self.console.read_recent_lines(state.read_mark) {
            state.read_mark = state.read_mark.saturating_add(1);
            self.mem_diag.record_line(&line);
            state.round_mem_diag.record_line(&line);
            state.ingest_line(&line, ssid_marker, require_listener);
        }
        Ok(state)
    }

    fn send_status_poll_if_ready(&mut self, state: &mut ProbeRoundState) {
        if Instant::now() >= state.next_status_poll {
            let _ = self.console.send_line("NET STATUS");
            state.next_status_poll =
                Instant::now() + Duration::from_millis(self.profile.status_poll_ms as u64);
        }
    }

    fn maybe_recover_after_round(&mut self, round: u32) -> Result<()> {
        wait_net_ack(&mut self.console, "NET RECOVER")
            .map_err(|err| anyhow!("round {round}: NET RECOVER after probe failed ({err})"))?;
        self.console.settle(self.profile.recover_settle_ms as u64)?;
        Ok(())
    }

    fn record_round_results(&mut self, round: u32, state: &ProbeRoundState, zero_discovery: bool) {
        if state.ready {
            self.ready_rounds = self.ready_rounds.saturating_add(1);
        }
        if zero_discovery {
            self.zero_discovery_rounds = self.zero_discovery_rounds.saturating_add(1);
        }
        if state.ssid_seen_events > 0 {
            self.ssid_seen_rounds = self.ssid_seen_rounds.saturating_add(1);
        }
        self.total_scan_zero_events = self
            .total_scan_zero_events
            .saturating_add(state.scan_zero_events);
        self.total_scan_nonzero_events = self
            .total_scan_nonzero_events
            .saturating_add(state.scan_nonzero_events);
        self.total_no_ap_found_events = self
            .total_no_ap_found_events
            .saturating_add(state.no_ap_found_events);

        self.samples.push(RoundSample {
            round,
            ready: state.ready,
            zero_discovery,
            scan_zero_events: state.scan_zero_events,
            scan_nonzero_events: state.scan_nonzero_events,
            no_ap_found_events: state.no_ap_found_events,
            ssid_seen_events: state.ssid_seen_events,
            failure_class: state.last_failure_class.clone(),
        });
        self.logger.info(format!(
            "round {}: ready={} zero_discovery={} scan_zero={} scan_nonzero={} no_ap_found={} ssid_seen={} failure_class={} min_internal_free={} min_total_free={} nomem_stage_samples={}",
            round,
            state.ready,
            zero_discovery,
            state.scan_zero_events,
            state.scan_nonzero_events,
            state.no_ap_found_events,
            state.ssid_seen_events,
            state.last_failure_class,
            fmt_min(&state.round_mem_diag.min_internal_free),
            fmt_min(&state.round_mem_diag.min_free),
            state.round_mem_diag.nomem_stage_samples
        ));
    }

    fn handle_advance_round(&mut self, context: &mut Value) -> Result<()> {
        let round = ctx_get_u32(context, "round")?;
        ctx_set_u32(context, "round", round.saturating_add(1))
    }

    fn handle_evaluate_results(&mut self, context: &mut Value) -> Result<()> {
        let pass_zero = self.zero_discovery_rounds <= self.profile.max_zero_discovery_rounds;
        let pass_ready = self.ready_rounds >= self.profile.min_ready_rounds;
        let observed_scan_activity =
            self.total_scan_zero_events > 0 || self.total_scan_nonzero_events > 0;
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

    fn handle_print_summary(&mut self) -> Result<()> {
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
        self.logger.info(format!(
            "summary mem samples={} radio_samples={} upload_samples={} nomem_stage_samples={} min_internal_free={} min_external_free={} min_total_free={} min_internal_low_water={}",
            self.mem_diag.samples,
            self.mem_diag.radio_samples,
            self.mem_diag.upload_samples,
            self.mem_diag.nomem_stage_samples,
            fmt_min(&self.mem_diag.min_internal_free),
            fmt_min(&self.mem_diag.min_external_free),
            fmt_min(&self.mem_diag.min_free),
            fmt_min(&self.mem_diag.min_internal_low_water),
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
        if self.profile.disable_listener_during_probe_rounds {
            if let Err(err) = wait_net_ack(&mut self.console, "NET LISTENER ON") {
                self.logger
                    .info(format!("summary: NET LISTENER ON restore failed ({err})"));
            }
        }
        Ok(())
    }

    fn handle_fail_run(&mut self, context: &mut Value) -> Result<()> {
        if self.profile.disable_listener_during_probe_rounds {
            if let Err(err) = wait_net_ack(&mut self.console, "NET LISTENER ON") {
                self.logger
                    .info(format!("fail_run: NET LISTENER ON restore failed ({err})"));
            }
        }
        let detail = ctx_get_string(context, "run_error")
            .unwrap_or_else(|_| "wifi discovery debug failed".to_string());
        Err(anyhow!("{detail}"))
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
    use super::{parse_mem_diag_line, parse_scan_done_count, MemDiagKind};

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
        assert_eq!(
            parse_scan_done_count("NET_STATUS {\"state\":\"Ready\"}"),
            None
        );
    }

    #[test]
    fn mem_diag_parser_extracts_radio_sample() {
        let line = "upload_http: radio_mem stage=scan_active_before trigger=none feature=true state=Initialized total=4259840 used=110160 free=4149680 peak=110160 internal_free=59280 external_free=4090400 min_free=4149680 min_internal_free=59280 min_external_free=4090400";
        let sample = parse_mem_diag_line(line).expect("radio sample parses");
        assert_eq!(sample.kind, MemDiagKind::Radio);
        assert_eq!(sample.stage, "scan_active_before");
        assert_eq!(sample.internal_free, 59280);
        assert_eq!(sample.min_internal_free, 59280);
    }
}
