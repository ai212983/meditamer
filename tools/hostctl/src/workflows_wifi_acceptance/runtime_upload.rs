use std::{
    fs, thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

use crate::{
    env_utils, workflows_upload,
    workflows_wifi_common::{
        ctx_get_string, ctx_get_u32, ctx_set_bool, ctx_set_string, ctx_set_u32, is_ready,
        query_net_status, query_net_status_line, PanicSignal,
    },
};

use super::WifiAcceptanceRuntime;

impl WifiAcceptanceRuntime<'_> {
    fn run_direct_upload_attempt(
        &mut self,
        ip: &str,
        cycle_root: &str,
        upload_timeout_sec: f64,
        retry_policy: workflows_upload::UploadRetryPolicy,
        force_fresh_client: bool,
    ) -> Result<()> {
        let opts = workflows_upload::DirectUploadOptions {
            host: ip,
            port: 8080,
            timeout_sec: upload_timeout_sec,
            src: &self.payload_path,
            dst_root: cycle_root,
            token: self.token.as_deref(),
            retry_policy,
        };
        if self.reuse_upload_client && !force_fresh_client {
            if self.upload_client.is_none() {
                self.upload_client = Some(workflows_upload::make_direct_upload_client(
                    upload_timeout_sec,
                )?);
            }
            let client = self
                .upload_client
                .as_ref()
                .ok_or_else(|| anyhow!("missing upload client"))?;
            return workflows_upload::upload_file_direct_fast_with_client(self.logger, client, opts);
        }

        let fresh_client = workflows_upload::make_direct_upload_client(upload_timeout_sec)?;
        let result = workflows_upload::upload_file_direct_fast_with_client(
            self.logger,
            &fresh_client,
            opts,
        );
        if self.reuse_upload_client && force_fresh_client && result.is_ok() {
            self.upload_client = Some(fresh_client);
        }
        result
    }

    pub(super) fn handle_assert_upload_metrics(&mut self, context: &mut Value) -> Result<()> {
        let current = self.query_req_read_body_reset()?;
        let baseline = self.req_read_body_reset_baseline.unwrap_or(current);
        let delta = current.saturating_sub(baseline);
        ctx_set_u32(context, "req_read_body_reset_delta", delta)?;
        ctx_set_u32(context, "req_read_body_reset_total", current)?;

        if delta > self.req_read_body_reset_max_delta {
            return Err(anyhow!(
                "upload metrics guard failed: req_read_body_reset delta={} exceeds max_delta={} (baseline={} current={})",
                delta,
                self.req_read_body_reset_max_delta,
                baseline,
                current
            ));
        }

        self.logger.info(format!(
            "upload_metrics_guard: req_read_body_reset delta={} max_delta={} baseline={} current={}",
            delta, self.req_read_body_reset_max_delta, baseline, current
        ));
        Ok(())
    }

    pub(super) fn handle_net_upload_once(&mut self, context: &mut Value) -> Result<()> {
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
        let upload_timeout_sec =
            env_utils::parse_env_f64("HOSTCTL_NET_UPLOAD_TIMEOUT_SEC", 180.0)?.max(1.0);
        let retry_policy = resolve_net_upload_retry_policy()?;
        let refresh_on_failure = refresh_upload_client_on_failure_enabled()?;
        let result =
            self.run_direct_upload_attempt(&ip, &cycle_root, upload_timeout_sec, retry_policy, false);
        let upload_ms = started.elapsed().as_millis() as u32;
        ctx_set_u32(context, "upload_ms", upload_ms)?;
        match result {
            Ok(()) => {
                ctx_set_bool(context, "upload_done", true)?;
                ctx_set_string(context, "upload_error", "")?;
            }
            Err(err) => {
                if self.reuse_upload_client {
                    // Drop pooled sockets after a failed cycle so retries/recovery start cleanly.
                    self.upload_client = None;
                }
                let mut detail = err.to_string();
                if detail.contains("health check failed: GET") {
                    let diag_message = append_health_fail_net_status(
                        &mut detail,
                        query_net_status_line(&mut self.console).map_err(|err| err.to_string()),
                    );
                    self.logger.info(diag_message);
                }
                let mut host_failure_class = classify_host_upload_failure(&detail);
                if refresh_on_failure
                    && host_failure_class.is_some_and(refresh_retry_eligible_host_failure)
                {
                    self.logger.info(format!(
                        "host_upload_refresh_retry: class={} cycle={} upload_attempt={} reuse_client={}",
                        host_failure_class.unwrap_or("unknown"),
                        ctx_get_u32(context, "cycle").unwrap_or(0),
                        ctx_get_u32(context, "upload_attempt").unwrap_or(0),
                        if self.reuse_upload_client { 1 } else { 0 },
                    ));
                    self.upload_client = None;
                    match self.run_direct_upload_attempt(
                        &ip,
                        &cycle_root,
                        upload_timeout_sec,
                        retry_policy,
                        true,
                    ) {
                        Ok(()) => {
                            let retry_upload_ms = started.elapsed().as_millis() as u32;
                            ctx_set_u32(context, "upload_ms", retry_upload_ms)?;
                            ctx_set_bool(context, "upload_done", true)?;
                            ctx_set_string(context, "upload_error", "")?;
                            self.logger.info(
                                "host_upload_refresh_retry: recovered upload with fresh client",
                            );
                            return Ok(());
                        }
                        Err(refresh_err) => {
                            detail.push_str(&format!(
                                "; refresh_retry_err={}",
                                refresh_err
                            ));
                            if detail.contains("health check failed: GET") {
                                let diag_message = append_health_fail_net_status(
                                    &mut detail,
                                    query_net_status_line(&mut self.console)
                                        .map_err(|err| err.to_string()),
                                );
                                self.logger.info(diag_message);
                            }
                            host_failure_class = classify_host_upload_failure(&detail);
                        }
                    }
                }

                if let Err(panic_err) = self.capture_mem_diag_lines() {
                    detail.push_str(&format!("; {panic_err}"));
                    ctx_set_bool(context, "upload_done", false)?;
                    ctx_set_string(context, "upload_error", &detail)?;
                    return Err(anyhow!("{detail}"));
                }

                if append_panic_signal_context(&mut detail, self.panic_signal()) {
                    ctx_set_bool(context, "upload_done", false)?;
                    ctx_set_string(context, "upload_error", &detail)?;
                    return Err(anyhow!("{detail}"));
                }

                if let Some(class) = host_failure_class {
                    self.logger
                        .info(format!("HOST_FAILURE class={class} detail={detail}"));
                }

                ctx_set_bool(context, "upload_done", false)?;
                ctx_set_string(context, "upload_error", &detail)?;
            }
        }
        Ok(())
    }

    pub(super) fn handle_net_verify_once(&mut self, context: &mut Value) -> Result<()> {
        let ip = ctx_get_string(context, "ip")?;
        let remote_file = ctx_get_string(context, "remote_file")?;
        let verify_timeout_sec =
            env_utils::parse_env_f64("HOSTCTL_NET_VERIFY_TIMEOUT_SEC", 30.0)?.max(0.5);
        let retry_policy = resolve_net_upload_retry_policy()?;
        if !workflows_upload::stat_remote_file(
            &ip,
            8080,
            verify_timeout_sec,
            &remote_file,
            self.token.as_deref(),
            retry_policy,
        )? {
            return Err(anyhow!("remote verify failed for {remote_file}"));
        }
        Ok(())
    }

    pub(super) fn handle_net_collect_diag(&mut self) -> Result<()> {
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

    pub(super) fn handle_net_recover_once(&mut self) -> Result<()> {
        if self.reuse_upload_client {
            // Network recovery may invalidate pooled sockets.
            self.upload_client = None;
        }
        self.send_net_command_best_effort("NET RECOVER");
        self.wait_recover_ready();
        if !self.is_recover_ready() {
            self.send_net_command_best_effort("NET LISTENER ON");
            thread::sleep(Duration::from_millis(120));
            self.send_net_command_best_effort("NET START");
            self.wait_recover_ready();
        }
        Ok(())
    }

    pub(super) fn handle_increment_upload_attempt(&self, context: &mut Value) -> Result<()> {
        let attempt = ctx_get_u32(context, "upload_attempt")?;
        ctx_set_u32(context, "upload_attempt", attempt.saturating_add(1))?;
        Ok(())
    }

    pub(super) fn handle_fail_upload(&mut self, context: &mut Value) -> Result<()> {
        self.log_mem_summary("failure summary");
        let detail = ctx_get_string(context, "upload_error")
            .unwrap_or_else(|_| "network/upload workflow failed".to_string());
        Err(anyhow!("{detail}"))
    }

    pub(super) fn handle_finalize_cycle(&mut self, context: &mut Value) -> Result<()> {
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

    pub(super) fn handle_advance_cycle(&self, context: &mut Value) -> Result<()> {
        let cycle = ctx_get_u32(context, "cycle")?;
        ctx_set_u32(context, "cycle", cycle.saturating_add(1))?;
        Ok(())
    }

    pub(super) fn handle_print_summary(&mut self) -> Result<()> {
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
        self.log_mem_summary("summary");
        Ok(())
    }
}

impl WifiAcceptanceRuntime<'_> {
    pub(super) fn query_req_read_body_reset(&mut self) -> Result<u32> {
        let mark = self.console.mark();
        self.console.send_line("METRICS")?;
        let re = Regex::new(r"^METRICS UPLOAD ")?;
        let line = self
            .console
            .wait_for_regex_since(mark, &re, Duration::from_secs(2))?
            .ok_or_else(|| anyhow!("missing METRICS UPLOAD response"))?;

        parse_metrics_key_u32(&line, "req_read_body_reset")
            .ok_or_else(|| anyhow!("METRICS UPLOAD missing req_read_body_reset: {line}"))
    }

    fn send_net_command_best_effort(&mut self, command: &str) {
        if let Err(err) = self.console.send_line(command) {
            self.logger.info(format!(
                "net_recover_once: failed to send {command} ({err}); continuing"
            ));
        }
    }

    fn wait_recover_ready(&mut self) {
        let ready_timeout_sec =
            env_utils::parse_env_f64("HOSTCTL_NET_RECOVER_READY_TIMEOUT_SEC", 12.0)
                .unwrap_or(12.0)
                .max(0.5);
        let poll_sec = env_utils::parse_env_f64("HOSTCTL_NET_RECOVER_READY_POLL_SEC", 0.4)
            .unwrap_or(0.4)
            .max(0.05);
        let deadline = Instant::now() + Duration::from_secs_f64(ready_timeout_sec);

        loop {
            if let Ok(Some(status)) = query_net_status(&mut self.console) {
                if is_ready(&status, true) {
                    return;
                }
            }
            if Instant::now() >= deadline {
                self.logger.info(format!(
                    "net_recover_once: ready wait timed out after {:.1}s; retrying upload anyway",
                    ready_timeout_sec
                ));
                return;
            }
            thread::sleep(Duration::from_secs_f64(poll_sec));
        }
    }

    fn is_recover_ready(&mut self) -> bool {
        match query_net_status(&mut self.console) {
            Ok(Some(status)) => is_ready(&status, true),
            Ok(None) => false,
            Err(err) => {
                self.logger.info(format!(
                    "net_recover_once: status query failed ({err}); treating as not ready"
                ));
                false
            }
        }
    }
}

fn resolve_net_upload_retry_policy() -> Result<workflows_upload::UploadRetryPolicy> {
    let default_sd_busy_retry_s =
        env_utils::parse_env_f64("HOSTCTL_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC", 30.0)?.max(1.0);
    let default_net_recovery_timeout_s =
        env_utils::parse_env_f64("HOSTCTL_UPLOAD_NET_RECOVERY_TIMEOUT_SEC", 8.0)?.max(0.1);
    let default_net_recovery_poll_s =
        env_utils::parse_env_f64("HOSTCTL_UPLOAD_NET_RECOVERY_POLL_SEC", 0.8)?.max(0.05);
    let default_net_recovery_consecutive_health =
        env_utils::parse_env_u32("HOSTCTL_UPLOAD_NET_RECOVERY_CONSECUTIVE_HEALTH", 2)?.max(1);
    Ok(workflows_upload::UploadRetryPolicy {
        sd_busy_total_retry_sec: env_utils::parse_env_f64(
            "HOSTCTL_NET_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC",
            default_sd_busy_retry_s,
        )?
        .max(1.0),
        net_recovery_timeout_sec: env_utils::parse_env_f64(
            "HOSTCTL_NET_UPLOAD_NET_RECOVERY_TIMEOUT_SEC",
            default_net_recovery_timeout_s,
        )?
        .max(0.1),
        net_recovery_poll_sec: env_utils::parse_env_f64(
            "HOSTCTL_NET_UPLOAD_NET_RECOVERY_POLL_SEC",
            default_net_recovery_poll_s,
        )?
        .max(0.05),
        net_recovery_consecutive_health_successes: env_utils::parse_env_u32(
            "HOSTCTL_NET_UPLOAD_NET_RECOVERY_CONSECUTIVE_HEALTH",
            default_net_recovery_consecutive_health,
        )?
        .max(1),
    })
}

fn append_health_fail_net_status(
    detail: &mut String,
    status_query: std::result::Result<Option<String>, String>,
) -> String {
    match status_query {
        Ok(Some(line)) => {
            detail.push_str(&format!("; net_status={line}"));
            format!("health_fail_diag: {line}")
        }
        Ok(None) => {
            detail.push_str("; net_status=<unavailable>");
            "health_fail_diag: NET_STATUS unavailable".to_string()
        }
        Err(err) => {
            detail.push_str(&format!("; net_status_query_error={err}"));
            format!("health_fail_diag: NET_STATUS query failed ({err})")
        }
    }
}

fn classify_host_upload_failure(detail: &str) -> Option<&'static str> {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("health check failed: get")
        && (lower.contains("send failed")
            || lower.contains("connection reset")
            || lower.contains("connection refused")
            || lower.contains("error sending request"))
    {
        return Some("host_health_send_fail");
    }
    if lower.contains("connection refused") {
        return Some("host_transport_connect_refused");
    }
    if lower.contains("send failed") || lower.contains("error sending request") {
        return Some("host_transport_send_fail");
    }
    if lower.contains("connection reset") || lower.contains("broken pipe") {
        return Some("host_transport_connection_reset");
    }
    None
}

fn refresh_retry_eligible_host_failure(class: &str) -> bool {
    matches!(
        class,
        "host_health_send_fail" | "host_transport_send_fail" | "host_transport_connection_reset"
    )
}

fn refresh_upload_client_on_failure_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_NET_UPLOAD_REFRESH_ON_FAILURE", true)
}

fn append_panic_signal_context(detail: &mut String, signal: Option<&PanicSignal>) -> bool {
    let Some(signal) = signal else {
        return false;
    };
    detail.push_str(&format!(
        "; panic_class={} panic_line_index={} panic_line={}",
        signal.class.as_str(),
        signal.marker_index,
        signal.marker_line
    ));
    true
}

fn avg(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn parse_metrics_key_u32(line: &str, key: &str) -> Option<u32> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
        .and_then(|value| value.parse::<u32>().ok())
}

#[cfg(test)]
mod tests {
    use super::{
        append_health_fail_net_status, append_panic_signal_context, classify_host_upload_failure,
        parse_metrics_key_u32, refresh_retry_eligible_host_failure,
    };
    use crate::workflows_wifi_common::detect_panic_signal;

    #[test]
    fn health_failure_detail_includes_net_status_snapshot() {
        let mut detail = "health check failed: GET http://10.0.0.8:8080/health".to_string();
        let diag = append_health_fail_net_status(
            &mut detail,
            Ok(Some(
                "NET_STATUS {\"state\":\"Ready\",\"link\":true,\"ipv4\":\"10.0.0.8\"}".to_string(),
            )),
        );
        assert!(diag.contains("health_fail_diag: NET_STATUS"));
        assert!(detail.contains("net_status=NET_STATUS"));
    }

    #[test]
    fn health_failure_detail_records_query_error_and_panic_context() {
        let mut detail = "health check failed: GET http://10.0.0.8:8080/health".to_string();
        let diag =
            append_health_fail_net_status(&mut detail, Err("serial read timed out".to_string()));
        assert!(diag.contains("NET_STATUS query failed"));
        assert!(detail.contains("net_status_query_error=serial read timed out"));

        let signal = detect_panic_signal("Guru Meditation Error: Core 0 panic'ed", 42)
            .expect("panic signal must be detected");
        assert!(append_panic_signal_context(&mut detail, Some(&signal)));
        assert!(detail.contains("panic_class=runtime_panic_guru"));
        assert!(detail.contains("panic_line_index=42"));
    }

    #[test]
    fn parse_req_read_body_reset_metric_from_upload_line() {
        let line = "METRICS UPLOAD accept_ok=10 accept_err=0 request_err=1 req_hdr_to=0 req_read_body=1 req_read_body_reset=1 req_sd_busy=0 sd_errors=0";
        assert_eq!(parse_metrics_key_u32(line, "req_read_body_reset"), Some(1));
        assert_eq!(parse_metrics_key_u32(line, "req_read_body"), Some(1));
        assert_eq!(parse_metrics_key_u32(line, "missing"), None);
    }

    #[test]
    fn classify_host_upload_failure_signatures() {
        assert_eq!(
            classify_host_upload_failure(
                "health check failed: GET http://10.0.0.8:8080/health last_error=GET ... send failed"
            ),
            Some("host_health_send_fail")
        );
        assert_eq!(
            classify_host_upload_failure(
                "PUT http://10.0.0.8:8080/upload send failed; connection refused (os error 61)"
            ),
            Some("host_transport_connect_refused")
        );
        assert_eq!(
            classify_host_upload_failure("PUT http://10.0.0.8:8080/upload send failed"),
            Some("host_transport_send_fail")
        );
        assert_eq!(
            classify_host_upload_failure("connection reset by peer"),
            Some("host_transport_connection_reset")
        );
        assert_eq!(classify_host_upload_failure("remote verify failed"), None);
    }

    #[test]
    fn refresh_retry_eligibility_covers_send_and_reset_classes() {
        assert!(refresh_retry_eligible_host_failure("host_health_send_fail"));
        assert!(refresh_retry_eligible_host_failure("host_transport_send_fail"));
        assert!(refresh_retry_eligible_host_failure(
            "host_transport_connection_reset"
        ));
        assert!(!refresh_retry_eligible_host_failure(
            "host_transport_connect_refused"
        ));
    }
}
