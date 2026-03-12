use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};

use crate::{
    serial_console::SerialConsole,
    workflows::wifi::common::{detect_panic_signal, query_net_status, NetPolicy, NetStatus},
};

fn format_failure(status: &NetStatus) -> String {
    format!(
        "network failure class={} code={} state={:?} ladder={:?} attempt={:?} listener_enabled={:?} uptime_ms={:?}",
        status.failure_class.as_deref().unwrap_or("unknown"),
        status.failure_code.unwrap_or_default(),
        status.state,
        status.ladder_step,
        status.attempt,
        status.listener_enabled,
        status.uptime_ms
    )
}

pub(super) fn wait_state_progress(console: &mut SerialConsole, timeout_ms: u32) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
    let mut read_mark = console.mark();
    while Instant::now() < deadline {
        if let Some(status) = query_net_status(console)? {
            let already_ready = matches!(status.state.as_deref(), Some("Ready"))
                && status.link.unwrap_or(false)
                && status.listener.unwrap_or(false)
                && status.ipv4.as_deref().is_some_and(|ip| ip != "0.0.0.0");
            if already_ready {
                return Ok(());
            }
            if let Some(
                "Recovering" | "Starting" | "Scanning" | "Associating" | "DhcpWait"
                | "ListenerWait" | "Ready",
            ) = status.state.as_deref()
            {
                return Ok(());
            }
        }
        detect_panic_since_mark(console, &mut read_mark)?;
        thread::sleep(Duration::from_millis(250));
    }
    Err(anyhow!("net_wait_state: did not leave idle state"))
}

struct WaitReadyState {
    started: Instant,
    post_connect_window_ms: u32,
    post_connect_deadline: Option<Instant>,
    last_attempt: Option<u64>,
    first_connect_ms: Option<u32>,
    last_nonterminal_failure: Option<NetStatus>,
    overall_deadline: Instant,
}

impl WaitReadyState {
    fn new(started: Instant, policy: NetPolicy) -> Self {
        let post_connect_window_ms = policy
            .dhcp_timeout_ms
            .saturating_add(policy.listener_timeout_ms)
            .saturating_add(2_000);
        let overall_deadline = started + Duration::from_millis(overall_ready_timeout_ms(policy));
        Self {
            started,
            post_connect_window_ms,
            post_connect_deadline: None,
            last_attempt: None,
            first_connect_ms: None,
            last_nonterminal_failure: None,
            overall_deadline,
        }
    }

    fn elapsed_ms(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    fn handle_status(&mut self, status: NetStatus) -> Result<Option<(u32, u32, String)>> {
        self.update_connect_timing(&status);
        if let Some(ip) = self.extract_listener_ip(&status) {
            let listen_ms = self.elapsed_ms();
            return Ok(Some((
                self.first_connect_ms
                    .unwrap_or_else(|| self.elapsed_ms())
                    .max(1),
                listen_ms,
                ip,
            )));
        }
        self.update_failure_if_any(status)?;
        Ok(None)
    }

    fn update_connect_timing(&mut self, status: &NetStatus) {
        let state = status.state.as_deref().unwrap_or("");
        let linked = status.link.unwrap_or(false);
        if let (Some(prev), Some(current)) = (self.last_attempt, status.attempt) {
            if current > prev {
                // Firmware advanced to a new ladder attempt; restart host listener window
                // so we do not fail before the new attempt can complete DHCP/listener stages.
                self.post_connect_deadline = None;
            }
        }
        self.last_attempt = status.attempt;
        if self.should_reset_post_connect_deadline(state) {
            self.post_connect_deadline = None;
        }
        if linked {
            if self.first_connect_ms.is_none() {
                self.first_connect_ms = Some(self.elapsed_ms());
            }
            if self.post_connect_deadline.is_none() {
                self.post_connect_deadline = Some(
                    Instant::now() + Duration::from_millis(self.post_connect_window_ms as u64),
                );
            }
        }
    }

    fn should_reset_post_connect_deadline(&self, state: &str) -> bool {
        matches!(
            state,
            "Recovering" | "Starting" | "Scanning" | "Associating"
        )
    }

    fn extract_listener_ip(&self, status: &NetStatus) -> Option<String> {
        let ipv4 = status.ipv4.as_deref()?;
        if !status.listener.unwrap_or(false)
            || !status.listener_enabled.unwrap_or(true)
            || ipv4 == "0.0.0.0"
        {
            return None;
        }
        Some(ipv4.to_string())
    }

    fn update_failure_if_any(&mut self, status: NetStatus) -> Result<()> {
        let failure_class = status.failure_class.as_deref().unwrap_or("none");
        if failure_class == "none" {
            return Ok(());
        }
        if self.is_terminal_failure(&status) {
            return Err(anyhow!("{}", format_failure(&status)));
        }
        self.last_nonterminal_failure = Some(status);
        Ok(())
    }

    fn is_terminal_failure(&self, status: &NetStatus) -> bool {
        matches!(status.state.as_deref(), Some("Failed"))
            || matches!(status.ladder_step.as_deref(), Some("terminal_fail"))
    }

    fn maybe_format_timeout(&self, prefix: &str) -> String {
        if let Some(status) = &self.last_nonterminal_failure {
            format!("net_wait_ready: {prefix} ({})", format_failure(status))
        } else {
            format!("net_wait_ready: {prefix}")
        }
    }

    fn check_timeouts(&self) -> Result<()> {
        if let Some(deadline) = self.post_connect_deadline {
            if Instant::now() > deadline {
                return Err(anyhow!("{}", self.maybe_format_timeout("listener timeout")));
            }
        }
        if Instant::now() > self.overall_deadline {
            return Err(anyhow!("{}", self.maybe_format_timeout("overall timeout")));
        }
        Ok(())
    }
}

pub(super) fn wait_ready(
    console: &mut SerialConsole,
    policy: NetPolicy,
) -> Result<(u32, u32, String)> {
    let started = Instant::now();
    let mut state = WaitReadyState::new(started, policy);
    let mut read_mark = console.mark();
    loop {
        if let Some(status) = query_net_status(console)? {
            if let Some(result) = state.handle_status(status)? {
                return Ok(result);
            }
        }
        detect_panic_since_mark(console, &mut read_mark)?;
        state.check_timeouts()?;
        thread::sleep(Duration::from_millis(350));
    }
}

fn detect_panic_since_mark(console: &SerialConsole, read_mark: &mut usize) -> Result<()> {
    for line in console.read_recent_lines(*read_mark) {
        let line_index = *read_mark;
        *read_mark = (*read_mark).saturating_add(1);
        if let Some(signal) = detect_panic_signal(&line, line_index) {
            return Err(anyhow!(
                "panic_detected class={} line_index={} line={}",
                signal.class.as_str(),
                signal.marker_index,
                signal.marker_line
            ));
        }
    }
    Ok(())
}

fn overall_ready_timeout_ms(policy: NetPolicy) -> u64 {
    let max_dhcp_ms = policy.dhcp_timeout_ms.max(policy.pinned_dhcp_timeout_ms) as u64;
    let scan_budget_ms = (policy.scan_active_max_ms.max(policy.scan_passive_ms) as u64)
        .saturating_mul(3)
        .saturating_add(policy.scan_passive_ms as u64);
    let per_attempt_ms = policy.connect_timeout_ms as u64
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
    per_attempt_ms
        .saturating_mul(ladder_attempts)
        .clamp(90_000, 420_000)
}

#[cfg(test)]
#[path = "wait_ready_tests.rs"]
mod tests;
