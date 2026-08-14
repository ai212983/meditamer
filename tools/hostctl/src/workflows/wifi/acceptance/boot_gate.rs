use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};

use crate::{
    logging::Logger,
    serial_console::SerialConsole,
    workflows::wifi::common::{
        is_ready, netcfg_set_payload, parse_net_status_line, parse_scan_done_count,
        query_net_status, wait_net_ack, NetPolicy, NetStatus,
    },
};

#[derive(Clone, Copy, Debug)]
pub(super) struct BootDiscoveryGateConfig {
    pub max_boot_uptime_ms: u32,
    pub timeout_ms: u32,
    pub settle_ms: u32,
    pub allow_ready_only_fallback: bool,
}

pub(super) fn run_boot_discovery_gate(
    logger: &mut Logger,
    console: &mut SerialConsole,
    ssid: &str,
    password: &str,
    policy: NetPolicy,
    cfg: BootDiscoveryGateConfig,
) -> Result<()> {
    let boot_status = query_boot_status_with_retry(console)?;
    let uptime_ms = boot_status.uptime_ms.unwrap_or(u64::MAX);
    if uptime_ms > cfg.max_boot_uptime_ms as u64 {
        logger.info(format!(
            "boot discovery gate: skipped (uptime_ms={} exceeds max_boot_uptime_ms={})",
            uptime_ms, cfg.max_boot_uptime_ms
        ));
        return Ok(());
    }

    logger.info(format!(
        "boot discovery gate: uptime_ms={} max_boot_uptime_ms={} timeout_ms={}",
        uptime_ms, cfg.max_boot_uptime_ms, cfg.timeout_ms
    ));

    let gate = run_boot_discovery_loop(logger, console, ssid, password, policy, cfg);
    let restore_listener = restore_listener_gate(logger, console);
    match (gate, restore_listener) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(gate_err), Ok(())) => Err(gate_err),
        (Ok(()), Err(restore_err)) => Err(anyhow!(
            "boot discovery gate passed but listener restore failed: {restore_err}"
        )),
        (Err(gate_err), Err(restore_err)) => Err(anyhow!(
            "boot discovery gate failed ({gate_err}); listener restore also failed ({restore_err})"
        )),
    }
}

fn restore_listener_gate(logger: &mut Logger, console: &mut SerialConsole) -> Result<()> {
    wait_net_ack(console, "NET LISTENER ON")?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Some(status) = query_net_status(console)? {
            if status.listener_enabled.unwrap_or(true) {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(180));
    }

    logger
        .info("boot discovery gate: listener_enabled stayed false after NET LISTENER ON; retrying");
    wait_net_ack(console, "NET LISTENER ON")?;
    let confirm_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < confirm_deadline {
        if let Some(status) = query_net_status(console)? {
            if status.listener_enabled.unwrap_or(true) {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(180));
    }

    Err(anyhow!(
        "boot discovery gate: NET LISTENER ON acked but listener_enabled remained false"
    ))
}

fn query_boot_status_with_retry(console: &mut SerialConsole) -> Result<NetStatus> {
    let mut boot_status = None;
    for _ in 0..8 {
        if let Some(status) = query_net_status(console)? {
            boot_status = Some(status);
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    boot_status.ok_or_else(|| anyhow!("boot discovery gate: unable to read NET STATUS"))
}

fn run_boot_discovery_loop(
    logger: &mut Logger,
    console: &mut SerialConsole,
    ssid: &str,
    password: &str,
    policy: NetPolicy,
    cfg: BootDiscoveryGateConfig,
) -> Result<()> {
    wait_net_ack(console, "NET LISTENER OFF")?;
    let payload = netcfg_set_payload(ssid, password, policy);
    wait_net_ack(console, &format!("NETCFG SET {payload}"))?;
    if let Err(err) = wait_net_ack(console, "NET RECOVER") {
        logger.info(format!(
            "boot discovery gate: NET RECOVER ack not obtained ({err}); continuing"
        ));
    }
    console.settle(cfg.settle_ms as u64)?;
    if let Err(err) = wait_net_ack(console, "NET START") {
        logger.info(format!(
            "boot discovery gate: NET START ack not obtained ({err}); continuing with status polling"
        ));
    }

    let mut state = BootDiscoveryLoopState::new(cfg);
    while Instant::now() < state.deadline {
        state.poll_lines(console, ssid)?;
        if state.reached_success() {
            logger.info(format!(
                "boot discovery gate: pass ready={} scan_nonzero_events={} ssid_seen_events={}",
                state.ready, state.scan_nonzero_events, state.ssid_seen_events
            ));
            return Ok(());
        }
        if state.reached_ready_ssid_fallback() {
            logger.info(format!(
                "boot discovery gate: pass via ready+ssid fallback ready={} scan_nonzero_events={} ssid_seen_events={}",
                state.ready, state.scan_nonzero_events, state.ssid_seen_events
            ));
            return Ok(());
        }
        if state.should_fallback(&cfg) {
            logger
                .info("boot discovery gate: pass via ready-only fallback (no fresh scan evidence)");
            return Ok(());
        }
        state.send_status_poll(console);
        thread::sleep(Duration::from_millis(250));
    }

    Err(anyhow!(
        "boot discovery gate failed: ready={} scan_nonzero_events={} ssid_seen_events={} timeout_ms={}",
        state.ready,
        state.scan_nonzero_events,
        state.ssid_seen_events,
        cfg.timeout_ms
    ))
}

struct BootDiscoveryLoopState {
    read_mark: usize,
    next_status_poll: Instant,
    deadline: Instant,
    gate_started: Instant,
    ready: bool,
    scan_nonzero_events: u32,
    ssid_seen_events: u32,
    ready_only_fallback_after: Duration,
    ready_ssid_fallback_after: Duration,
}

impl BootDiscoveryLoopState {
    fn new(cfg: BootDiscoveryGateConfig) -> Self {
        Self {
            read_mark: 0,
            next_status_poll: Instant::now(),
            deadline: Instant::now() + Duration::from_millis(cfg.timeout_ms as u64),
            gate_started: Instant::now(),
            ready: false,
            scan_nonzero_events: 0,
            ssid_seen_events: 0,
            ready_only_fallback_after: Duration::from_millis(8_000),
            ready_ssid_fallback_after: Duration::from_millis(2_500),
        }
    }

    fn poll_lines(&mut self, console: &mut SerialConsole, ssid: &str) -> Result<()> {
        console.poll_once()?;
        for line in console.read_recent_lines(self.read_mark) {
            self.read_mark = self.read_mark.saturating_add(1);
            self.observe_boot_line(&line, ssid);
        }
        Ok(())
    }

    fn observe_boot_line(&mut self, line: &str, ssid: &str) {
        if let Some(count) = parse_scan_done_count(line) {
            if count > 0 {
                self.scan_nonzero_events = self.scan_nonzero_events.saturating_add(1);
            }
        }
        if line.starts_with("upload_http: scan ap ssid=")
            && line.contains(&format!("scan ap ssid={ssid}"))
        {
            self.ssid_seen_events = self.ssid_seen_events.saturating_add(1);
        }
        if line.starts_with("NET_STATUS ") {
            if let Ok(status) = parse_net_status_line(line) {
                if is_ready(&status, false) {
                    self.ready = true;
                }
            }
        }
    }

    fn reached_success(&self) -> bool {
        self.ready && self.scan_nonzero_events > 0 && self.ssid_seen_events > 0
    }

    fn reached_ready_ssid_fallback(&self) -> bool {
        self.ready
            && self.ssid_seen_events > 0
            && self.scan_nonzero_events == 0
            && self.gate_started.elapsed() >= self.ready_ssid_fallback_after
    }

    fn should_fallback(&self, cfg: &BootDiscoveryGateConfig) -> bool {
        self.ready
            && self.scan_nonzero_events == 0
            && self.ssid_seen_events == 0
            && self.gate_started.elapsed() >= self.ready_only_fallback_after
            && cfg.allow_ready_only_fallback
    }

    fn send_status_poll(&mut self, console: &mut SerialConsole) {
        if Instant::now() >= self.next_status_poll {
            let _ = console.send_line("NET STATUS");
            self.next_status_poll = Instant::now() + Duration::from_millis(1_000);
        }
    }
}

#[cfg(test)]
#[path = "boot_gate_tests.rs"]
mod tests;
