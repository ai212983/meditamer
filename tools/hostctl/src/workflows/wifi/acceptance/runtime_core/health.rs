use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use reqwest::StatusCode;

use crate::{env_utils, workflows::wifi::common::NetStatus};

use super::super::{wait_ready::wait_ready, WifiAcceptanceRuntime};

impl WifiAcceptanceRuntime<'_> {
    pub(super) fn enforce_startup_health_hysteresis(
        &mut self,
        mut connect_ms: u32,
        mut listen_ms: u32,
        ip: &str,
    ) -> Result<(u32, u32, String)> {
        if !env_utils::parse_env_bool01("HOSTCTL_NET_STARTUP_HEALTH_HYSTERESIS", true)? {
            return Ok((connect_ms, listen_ms, ip.to_string()));
        }
        let required_streak =
            env_utils::parse_env_u32("HOSTCTL_NET_STARTUP_HEALTH_SUCCESS_STREAK", 3)?.max(1);
        let req_timeout_s =
            env_utils::parse_env_f64("HOSTCTL_NET_STARTUP_HEALTH_REQ_TIMEOUT_SEC", 1.5)?
                .clamp(0.2, 5.0);
        let window_timeout_s =
            env_utils::parse_env_f64("HOSTCTL_NET_STARTUP_HEALTH_HYSTERESIS_TIMEOUT_SEC", 20.0)?
                .max(1.0);
        let poll_ms = env_utils::parse_env_u64("HOSTCTL_NET_STARTUP_HEALTH_POLL_MS", 300)?.max(50);
        let recover_retries =
            env_utils::parse_env_u32("HOSTCTL_NET_STARTUP_HEALTH_RECOVER_RETRIES", 1)?;

        let mut recover_attempt = 0u32;
        let mut stabilized_ip = ip.to_string();
        loop {
            match wait_startup_health_streak(
                &stabilized_ip,
                required_streak,
                req_timeout_s,
                window_timeout_s,
                poll_ms,
            ) {
                Ok((attempts, elapsed_ms)) => {
                    self.logger.info(format!(
                        "startup health hysteresis: pass cycle=1 ip={} required_streak={} attempts={} elapsed_ms={}",
                        stabilized_ip, required_streak, attempts, elapsed_ms
                    ));
                    return Ok((connect_ms, listen_ms, stabilized_ip));
                }
                Err(err) if recover_attempt < recover_retries => {
                    recover_attempt = recover_attempt.saturating_add(1);
                    self.logger.info(format!(
                        "startup health hysteresis: fail attempt={} err={}; issuing NET RECOVER and re-checking ready",
                        recover_attempt, err
                    ));
                    self.handle_net_recover_once()?;
                    let (reconnect_ms, relisten_ms, recovered_ip) =
                        wait_ready(&mut self.console, self.policy)?;
                    connect_ms = connect_ms.saturating_add(reconnect_ms);
                    listen_ms = listen_ms.saturating_add(relisten_ms);
                    stabilized_ip = recovered_ip;
                }
                Err(err) => {
                    return Err(anyhow!(
                        "startup health hysteresis failed after {} recover attempt(s): {err}",
                        recover_attempt
                    ));
                }
            }
        }
    }
}

pub(super) fn should_force_recover_before_start(status: &NetStatus) -> bool {
    matches!(
        status.state.as_deref(),
        Some(
            "Recovering"
                | "Starting"
                | "Scanning"
                | "Associating"
                | "DhcpWait"
                | "ListenerWait"
                | "Failed"
        )
    )
}

pub(super) fn is_ready_without_listener(status: &NetStatus) -> bool {
    matches!(status.state.as_deref(), Some("Ready"))
        && status.link.unwrap_or(false)
        && status.ipv4.as_deref().is_some_and(|ip| ip != "0.0.0.0")
        && status.listener_enabled.unwrap_or(true)
        && !status.listener.unwrap_or(false)
}

pub(super) fn should_retry_wait_ready_after_recover(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("failure class=listener_not_ready")
        || lower.contains("dhcp_no_ipv4_stall")
        || lower.contains("dhcp/no-ipv4 stall")
        || lower.contains("net_wait_ready: listener timeout")
        || lower.contains("listener timeout")
}

fn wait_startup_health_streak(
    ip: &str,
    required_streak: u32,
    req_timeout_s: f64,
    window_timeout_s: f64,
    poll_ms: u64,
) -> Result<(u32, u32)> {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs_f64(req_timeout_s))
        .timeout(Duration::from_secs_f64(req_timeout_s))
        .build()?;
    let health_url = format!("http://{ip}:8080/health");
    let started = Instant::now();
    let deadline = started + Duration::from_secs_f64(window_timeout_s);
    let mut streak = 0u32;
    let mut attempts = 0u32;
    let mut last_error = String::from("<none>");

    while Instant::now() < deadline {
        attempts = attempts.saturating_add(1);
        match client.get(&health_url).send() {
            Ok(response) if response.status().is_success() => {
                streak = streak.saturating_add(1);
                if streak >= required_streak {
                    return Ok((attempts, started.elapsed().as_millis() as u32));
                }
            }
            Ok(response) => {
                streak = 0;
                last_error = format_health_status_error(response.status());
            }
            Err(err) => {
                streak = 0;
                last_error = err.to_string();
            }
        }
        thread::sleep(Duration::from_millis(poll_ms));
    }

    Err(anyhow!(
        "GET {health_url} startup health streak timeout required_streak={} attempts={} last_error={}",
        required_streak,
        attempts,
        last_error
    ))
}

pub(super) fn format_health_status_error(status: StatusCode) -> String {
    format!("HTTP {}", status.as_u16())
}
