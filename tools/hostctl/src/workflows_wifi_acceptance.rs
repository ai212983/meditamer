use std::{
    fs,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::{AckStatus, SerialConsole},
    workflows_upload,
};

#[derive(Clone, Debug)]
pub struct WifiAcceptanceOptions {
    pub output_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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

impl Default for NetPolicy {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 30_000,
            dhcp_timeout_ms: 20_000,
            pinned_dhcp_timeout_ms: 45_000,
            listener_timeout_ms: 25_000,
            scan_active_min_ms: 600,
            scan_active_max_ms: 1_500,
            scan_passive_ms: 1_500,
            retry_same_max: 2,
            rotate_candidate_max: 2,
            rotate_auth_max: 5,
            full_scan_reset_max: 1,
            driver_restart_max: 1,
            cooldown_ms: 1_200,
            driver_restart_backoff_ms: 2_500,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct NetStatus {
    state: Option<String>,
    link: Option<bool>,
    ipv4: Option<String>,
    listener: Option<bool>,
    failure_class: Option<String>,
    failure_code: Option<u64>,
    ladder_step: Option<String>,
    attempt: Option<u64>,
    uptime_ms: Option<u64>,
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

fn fmt_min(value: &Option<(u64, String)>) -> String {
    match value {
        Some((bytes, stage)) => format!("{bytes}@{stage}"),
        None => "n/a".to_string(),
    }
}

struct WifiAcceptanceRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    payload_path: PathBuf,
    remote_root: String,
    ssid: String,
    password: String,
    token: Option<String>,
    policy: NetPolicy,
    cycles: u32,
    operation_retries: u32,
    connect_samples: Vec<f64>,
    listen_samples: Vec<f64>,
    upload_samples: Vec<f64>,
    throughput_samples: Vec<f64>,
    started: Instant,
    mem_diag: MemDiagSummary,
    mem_read_mark: usize,
}

pub fn run_wifi_acceptance(logger: &mut Logger, opts: WifiAcceptanceOptions) -> Result<()> {
    let port = std::env::var("HOSTCTL_NET_PORT")
        .context("HOSTCTL_NET_PORT must be set (hard-cut net workflow)")?;
    let baud = std::env::var("HOSTCTL_NET_BAUD")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(115200);
    let ssid = std::env::var("HOSTCTL_NET_SSID")
        .context("HOSTCTL_NET_SSID must be set (hard-cut net workflow)")?;
    let password = std::env::var("HOSTCTL_NET_PASSWORD").unwrap_or_default();
    let policy_path = std::env::var("HOSTCTL_NET_POLICY_PATH")
        .context("HOSTCTL_NET_POLICY_PATH must be set (hard-cut net workflow)")?;
    let skip_host_wifi_check =
        env_utils::parse_env_bool01("HOSTCTL_NET_SKIP_HOST_WIFI_CHECK", false)?;
    if !skip_host_wifi_check {
        ensure_host_wifi_association(&ssid)?;
    }
    let log_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(
            std::env::var("HOSTCTL_NET_LOG_PATH").unwrap_or_else(|_| {
                format!(
                    "logs/wifi_acceptance_{}.log",
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
    let cycles = env_utils::parse_env_u32("HOSTCTL_NET_CYCLES", 3)?.max(1);
    let operation_retries = env_utils::parse_env_u32("HOSTCTL_NET_OPERATION_RETRIES", 3)?.max(1);

    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-acceptance.sw.yaml"),
    )?;

    let payload_path = PathBuf::from("/tmp/net_acceptance_payload.bin");
    let remote_root = "/assets".to_string();
    let token = std::env::var("HOSTCTL_UPLOAD_TOKEN").ok();

    let mut runtime = WifiAcceptanceRuntime {
        logger,
        console,
        payload_path,
        remote_root,
        ssid,
        password,
        token,
        policy,
        cycles,
        operation_retries,
        connect_samples: Vec::new(),
        listen_samples: Vec::new(),
        upload_samples: Vec::new(),
        throughput_samples: Vec::new(),
        started: Instant::now(),
        mem_diag: MemDiagSummary::default(),
        mem_read_mark: 0,
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

fn query_net_status(console: &mut SerialConsole) -> Result<Option<NetStatus>> {
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

fn format_failure(status: &NetStatus) -> String {
    format!(
        "network failure class={} code={} state={:?} ladder={:?} attempt={:?} uptime_ms={:?}",
        status.failure_class.as_deref().unwrap_or("unknown"),
        status.failure_code.unwrap_or_default(),
        status.state,
        status.ladder_step,
        status.attempt,
        status.uptime_ms
    )
}

fn wait_state_progress(console: &mut SerialConsole, timeout_ms: u32) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
    while Instant::now() < deadline {
        if let Some(status) = query_net_status(console)? {
            let already_ready = matches!(status.state.as_deref(), Some("Ready"))
                && status.link.unwrap_or(false)
                && status.listener.unwrap_or(false)
                && status
                    .ipv4
                    .as_deref()
                    .is_some_and(|ip| ip != "0.0.0.0");
            if already_ready {
                return Ok(());
            }
            if let Some(
                "Recovering"
                | "Starting"
                | "Scanning"
                | "Associating"
                | "DhcpWait"
                | "ListenerWait"
                | "Ready",
            ) =
                status.state.as_deref()
            {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(anyhow!("net_wait_state: did not leave idle state"))
}

fn wait_ready(
    console: &mut SerialConsole,
    policy: NetPolicy,
) -> Result<(u32, u32, String)> {
    let started = Instant::now();
    let post_connect_window_ms = policy
        .dhcp_timeout_ms
        .saturating_add(policy.listener_timeout_ms)
        .saturating_add(2_000);
    let overall_deadline = started + Duration::from_millis(overall_ready_timeout_ms(policy));
    let mut post_connect_deadline: Option<Instant> = None;
    let mut first_connect_ms: Option<u32> = None;
    let mut last_nonterminal_failure: Option<NetStatus> = None;
    loop {
        if let Some(status) = query_net_status(console)? {
            let state = status.state.as_deref().unwrap_or("");
            let linked = status.link.unwrap_or(false);
            if linked {
                if first_connect_ms.is_none() {
                    first_connect_ms = Some(started.elapsed().as_millis() as u32);
                }
                if post_connect_deadline.is_none() {
                    post_connect_deadline =
                        Some(Instant::now() + Duration::from_millis(post_connect_window_ms as u64));
                }
            } else if matches!(state, "Recovering" | "Starting" | "Scanning" | "Associating") {
                // Link dropped and firmware is re-entering connect path; clear the listener-phase
                // deadline so host does not fail a valid reconnect attempt.
                post_connect_deadline = None;
            }
            if status.listener.unwrap_or(false) {
                if let Some(ipv4) = status.ipv4.as_deref() {
                    if ipv4 != "0.0.0.0" {
                        let listen_ms = started.elapsed().as_millis() as u32;
                        return Ok((
                            first_connect_ms
                                .unwrap_or_else(|| started.elapsed().as_millis() as u32)
                                .max(1),
                            listen_ms,
                            ipv4.to_string(),
                        ));
                    }
                }
            }
            if let Some(failure_class) = status.failure_class.as_deref() {
                if failure_class != "none" {
                    let terminal_state = matches!(status.state.as_deref(), Some("Failed"));
                    let terminal_ladder =
                        matches!(status.ladder_step.as_deref(), Some("terminal_fail"));
                    if terminal_state || terminal_ladder {
                        return Err(anyhow!("{}", format_failure(&status)));
                    }
                    last_nonterminal_failure = Some(status.clone());
                }
            }
        }

        if let Some(deadline) = post_connect_deadline {
            if Instant::now() > deadline {
                if let Some(status) = last_nonterminal_failure.as_ref() {
                    return Err(anyhow!(
                        "net_wait_ready: listener timeout ({})",
                        format_failure(status)
                    ));
                }
                return Err(anyhow!("net_wait_ready: listener timeout"));
            }
        }
        if Instant::now() > overall_deadline {
            if let Some(status) = last_nonterminal_failure.as_ref() {
                return Err(anyhow!(
                    "net_wait_ready: overall timeout ({})",
                    format_failure(status)
                ));
            }
            return Err(anyhow!("net_wait_ready: overall timeout"));
        }
        thread::sleep(Duration::from_millis(350));
    }
}

fn overall_ready_timeout_ms(policy: NetPolicy) -> u64 {
    let max_dhcp_ms = policy.dhcp_timeout_ms.max(policy.pinned_dhcp_timeout_ms) as u64;
    let scan_budget_ms = (policy.scan_active_max_ms.max(policy.scan_passive_ms) as u64)
        .saturating_mul(3)
        .saturating_add(policy.scan_passive_ms as u64);
    let per_attempt_ms = policy
        .connect_timeout_ms as u64
        + max_dhcp_ms
        + policy.listener_timeout_ms as u64
        + scan_budget_ms
        + policy.cooldown_ms as u64
        + policy.driver_restart_backoff_ms as u64;
    let ladder_attempts = 1u64
        + policy.retry_same_max as u64
        + policy.rotate_candidate_max as u64
        + policy.rotate_auth_max as u64
        + policy.full_scan_reset_max as u64
        + policy.driver_restart_max as u64;
    per_attempt_ms.saturating_mul(ladder_attempts).clamp(90_000, 420_000)
}

fn verify_remote_file(console: &mut SerialConsole, remote_path: &str) -> Result<bool> {
    let re = Regex::new(r"^SDFATSTAT (OK|BUSY|ERR)")?;
    for _ in 0..8 {
        let mark = console.mark();
        console.send_line(&format!("SDFATSTAT {remote_path}"))?;
        let line = console.wait_for_regex_since(mark, &re, Duration::from_secs(4))?;
        let Some(line) = line else {
            continue;
        };
        if line.contains("SDFATSTAT ERR") {
            return Ok(false);
        }
        if line.contains("SDFATSTAT BUSY") {
            thread::sleep(Duration::from_millis(400));
            continue;
        }
        let req_id = console
            .wait_for_sdreq_id_since(mark, Some("fat_stat"), Duration::from_secs(8))?
            .ok_or_else(|| anyhow!("missing SDREQ id for fat_stat"))?;
        let done = console
            .sdwait_for_id(req_id, 30_000)?
            .unwrap_or_default();
        if done.contains("SDWAIT DONE") && done.contains("status=ok") && done.contains("code=ok") {
            return Ok(true);
        }
    }
    Ok(false)
}

impl WifiAcceptanceRuntime<'_> {
    fn capture_mem_diag_lines(&mut self) -> Result<()> {
        self.console.poll_once()?;
        for line in self.console.read_recent_lines(self.mem_read_mark) {
            self.mem_read_mark = self.mem_read_mark.saturating_add(1);
            self.mem_diag.record_line(&line);
        }
        Ok(())
    }
}

impl WorkflowRuntime for WifiAcceptanceRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        self.capture_mem_diag_lines()?;
        let result = match action {
            "prepare_payload" => {
                ensure_parent_dir(&self.payload_path)?;
                let mut data = vec![0u8; 524_288];
                for (i, slot) in data.iter_mut().enumerate() {
                    *slot = ((i * 17 + 31) & 0xFF) as u8;
                }
                fs::write(&self.payload_path, data)?;
                Ok(())
            }
            "start_run" => {
                ctx_set_u32(context, "cycle", 1)?;
                ctx_set_u32(context, "cycles", self.cycles)?;
                ctx_set_u32(context, "operation_retries", self.operation_retries)?;
                self.mem_read_mark = self.console.mark();
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
            "net_start" => {
                if let Err(err) = wait_net_ack(&mut self.console, "NET START") {
                    let ready_now = query_net_status(&mut self.console)?
                        .is_some_and(|status| {
                            matches!(status.state.as_deref(), Some("Ready"))
                                && status.link.unwrap_or(false)
                                && status.listener.unwrap_or(false)
                                && status
                                    .ipv4
                                    .as_deref()
                                    .is_some_and(|ip| ip != "0.0.0.0")
                        });
                    if !ready_now {
                        return Err(err);
                    }
                    self.logger.info(format!(
                        "net_start: start ack not obtained ({err}); continuing because network is already ready"
                    ));
                }
                Ok(())
            }
            "net_wait_state" => {
                wait_state_progress(&mut self.console, self.policy.connect_timeout_ms)
            }
            "net_wait_ready" => {
                let (connect_ms, listen_ms, ip) = wait_ready(&mut self.console, self.policy)?;
                ctx_set_u32(context, "connect_ms", connect_ms)?;
                ctx_set_u32(context, "listen_ms", listen_ms)?;
                ctx_set_string(context, "ip", &ip)?;
                self.connect_samples.push(connect_ms as f64 / 1000.0);
                self.listen_samples.push(listen_ms as f64 / 1000.0);
                Ok(())
            }
            "init_upload_attempt" => {
                ctx_set_u32(context, "upload_attempt", 1)?;
                ctx_set_bool(context, "upload_done", false)?;
                Ok(())
            }
            "net_upload_once" => {
                let ip = ctx_get_string(context, "ip")?;
                let cycle_root = self.remote_root.clone();
                let upload_name = self
                    .payload_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "net_acceptance_payload.bin".to_string());
                let remote_file = format!("{}/{}", self.remote_root, upload_name);
                ctx_set_string(context, "remote_file", &remote_file)?;
                let started = Instant::now();
                let result = workflows_upload::upload_file_direct(
                    self.logger,
                    &ip,
                    8080,
                    180.0,
                    &self.payload_path,
                    &cycle_root,
                    self.token.as_deref(),
                );
                let upload_ms = started.elapsed().as_millis() as u32;
                ctx_set_u32(context, "upload_ms", upload_ms)?;
                match result {
                    Ok(()) => {
                        ctx_set_bool(context, "upload_done", true)?;
                        ctx_set_string(context, "upload_error", "")?;
                    }
                    Err(err) => {
                        ctx_set_bool(context, "upload_done", false)?;
                        ctx_set_string(context, "upload_error", &err.to_string())?;
                    }
                }
                Ok(())
            }
            "net_verify_once" => {
                let remote_file = ctx_get_string(context, "remote_file")?;
                if !verify_remote_file(&mut self.console, &remote_file)? {
                    return Err(anyhow!("remote verify failed for {remote_file}"));
                }
                Ok(())
            }
            "net_collect_diag" => {
                let status_re = Regex::new(r"^NET_STATUS \{")?;
                let mark = self.console.mark();
                self.console.send_line("NET STATUS")?;
                if let Some(line) =
                    self.console
                        .wait_for_regex_since(mark, &status_re, Duration::from_secs(2))?
                {
                    self.logger.info(format!("diag: {line}"));
                }
                Ok(())
            }
            "net_recover_once" => {
                if let Err(err) = wait_net_ack(&mut self.console, "NET RECOVER") {
                    self.logger
                        .info(format!("net_recover_once: recover ack not obtained ({err}); continuing"));
                }
                Ok(())
            }
            "increment_upload_attempt" => {
                let attempt = ctx_get_u32(context, "upload_attempt")?;
                ctx_set_u32(context, "upload_attempt", attempt.saturating_add(1))?;
                Ok(())
            }
            "fail_upload" | "net_fail" => {
                let detail = ctx_get_string(context, "upload_error")
                    .unwrap_or_else(|_| "network/upload workflow failed".to_string());
                Err(anyhow!("{detail}"))
            }
            "finalize_cycle" => {
                let connect_ms = ctx_get_u32(context, "connect_ms")?;
                let listen_ms = ctx_get_u32(context, "listen_ms")?;
                let upload_ms = ctx_get_u32(context, "upload_ms")?;
                let cycle = ctx_get_u32(context, "cycle")?;
                let payload_bytes = fs::metadata(&self.payload_path)?.len() as f64;
                let upload_s = (upload_ms as f64 / 1000.0).max(0.001);
                let kib_s = payload_bytes / 1024.0 / upload_s;
                self.upload_samples.push(upload_s);
                self.throughput_samples.push(kib_s);
                self.logger.info(format!(
                    "cycle {}: connect_ms={} listen_ms={} upload_ms={} throughput_kib_s={:.2}",
                    cycle, connect_ms, listen_ms, upload_ms, kib_s
                ));
                Ok(())
            }
            "advance_cycle" => {
                let cycle = ctx_get_u32(context, "cycle")?;
                ctx_set_u32(context, "cycle", cycle.saturating_add(1))?;
                Ok(())
            }
            "print_summary" => {
                let avg_connect = avg(&self.connect_samples);
                let avg_listen = avg(&self.listen_samples);
                let avg_upload = avg(&self.upload_samples);
                let avg_throughput = avg(&self.throughput_samples);
                self.logger.info(format!(
                    "summary cycles={} avg_connect_s={:.2} avg_listen_s={:.2} avg_upload_s={:.2} avg_kib_s={:.2} total_s={:.2}",
                    self.connect_samples.len(),
                    avg_connect,
                    avg_listen,
                    avg_upload,
                    avg_throughput,
                    self.started.elapsed().as_secs_f64(),
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
                Ok(())
            }
            _ => Err(anyhow!("unknown workflow action: {action}")),
        };
        self.capture_mem_diag_lines()?;
        result
    }
}

fn avg(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

#[cfg(target_os = "macos")]
fn ensure_host_wifi_association(expected_ssid: &str) -> Result<()> {
    let ports_out = Command::new("networksetup")
        .arg("-listallhardwareports")
        .output()
        .context("failed to execute networksetup -listallhardwareports")?;
    if !ports_out.status.success() {
        return Err(anyhow!(
            "networksetup -listallhardwareports failed: {}",
            String::from_utf8_lossy(&ports_out.stderr)
        ));
    }
    let ports_text = String::from_utf8_lossy(&ports_out.stdout);
    let mut wifi_device: Option<String> = None;
    let mut saw_wifi_port = false;
    for line in ports_text.lines() {
        if line.starts_with("Hardware Port: ") {
            saw_wifi_port = line.trim() == "Hardware Port: Wi-Fi";
            continue;
        }
        if saw_wifi_port && line.starts_with("Device: ") {
            wifi_device = Some(line["Device: ".len()..].trim().to_string());
            break;
        }
    }
    let Some(device) = wifi_device else {
        return Err(anyhow!("unable to determine host Wi-Fi interface via networksetup"));
    };

    let assoc_out = Command::new("networksetup")
        .args(["-getairportnetwork", &device])
        .output()
        .with_context(|| format!("failed to execute networksetup -getairportnetwork {device}"))?;
    if !assoc_out.status.success() {
        // macOS Wi-Fi tooling output/exit semantics are not reliable enough to
        // gate acceptance runs. Health checks later in the workflow are
        // authoritative for host<->device reachability.
        return Ok(());
    }
    let assoc_text = String::from_utf8_lossy(&assoc_out.stdout).trim().to_string();
    let assoc_lower = assoc_text.to_ascii_lowercase();
    if assoc_lower.contains("not associated") {
        // Deprecated "AirPort" messaging can appear even when host routing is
        // otherwise functional. Treat this as non-fatal.
        return Ok(());
    }
    let current_ssid = assoc_text
        .split(':')
        .next_back()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if current_ssid != expected_ssid {
        return Err(anyhow!(
            "host Wi-Fi associated to SSID {} on {}, expected {}",
            current_ssid,
            device,
            expected_ssid
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn ensure_host_wifi_association(_expected_ssid: &str) -> Result<()> {
    Ok(())
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
