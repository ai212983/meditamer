//! Upload cycle for the Wi-Fi acceptance run.
//!
mod cycle;
mod helpers;

use helpers::{
    append_health_fail_net_status, append_panic_signal_context, classify_host_upload_failure,
    refresh_retry_eligible_host_failure, refresh_upload_client_on_failure_enabled,
    resolve_net_upload_retry_policy,
};

use std::time::Instant;

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::workflows::{
    upload,
    wifi::common::{ctx_get_string, ctx_get_u32, query_net_status_line},
};

use super::WifiAcceptanceRuntime;

impl WifiAcceptanceRuntime<'_> {
    fn upload_attempt_result(
        &self,
        remote_file: &str,
        upload_ms: u32,
        upload_done: bool,
        upload_error: &str,
    ) -> Value {
        serde_json::json!({
            "remote_file": remote_file,
            "upload_ms": upload_ms,
            "upload_done": upload_done,
            "upload_error": upload_error
        })
    }

    fn run_direct_upload_attempt(
        &mut self,
        ip: &str,
        cycle_root: &str,
        upload_timeout_sec: f64,
        retry_policy: upload::UploadRetryPolicy,
        force_fresh_client: bool,
    ) -> Result<()> {
        let opts = upload::DirectUploadOptions {
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
                self.upload_client = Some(upload::make_direct_upload_client(upload_timeout_sec)?);
            }
            let client = self
                .upload_client
                .as_ref()
                .ok_or_else(|| anyhow!("missing upload client"))?;
            return upload::upload_file_direct_fast_with_client(self.logger, client, opts);
        }

        let fresh_client = upload::make_direct_upload_client(upload_timeout_sec)?;
        let result = upload::upload_file_direct_fast_with_client(self.logger, &fresh_client, opts);
        if self.reuse_upload_client && force_fresh_client && result.is_ok() {
            self.upload_client = Some(fresh_client);
        }
        result
    }

    fn upload_metrics_result(&self, delta: u32, current: u32) -> Value {
        serde_json::json!({
            "req_read_body_reset_delta": delta,
            "req_read_body_reset_total": current
        })
    }

    pub(super) fn handle_assert_upload_metrics(&mut self) -> Result<Value> {
        let current = self.query_req_read_body_reset()?;
        let baseline = self.req_read_body_reset_baseline.unwrap_or(current);
        let delta = current.saturating_sub(baseline);

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
        Ok(self.upload_metrics_result(delta, current))
    }

    pub(super) fn handle_net_upload_once(&mut self, context: &mut Value) -> Result<Value> {
        let ip = ctx_get_string(context, "ip")?;
        let cycle_root = self.remote_root.clone();
        let upload_name = self
            .payload_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "net_acceptance_payload.bin".to_string());
        let remote_file = format!("{}/{}", self.remote_root, upload_name);
        let started = Instant::now();
        let upload_timeout_sec = 180.0f64.max(1.0);
        let retry_policy = resolve_net_upload_retry_policy()?;
        let refresh_on_failure = refresh_upload_client_on_failure_enabled();
        let result = self.run_direct_upload_attempt(
            &ip,
            &cycle_root,
            upload_timeout_sec,
            retry_policy,
            false,
        );
        match result {
            Ok(()) => Ok(self.upload_attempt_result(
                &remote_file,
                started.elapsed().as_millis() as u32,
                true,
                "",
            )),
            Err(err) => {
                if self.reuse_upload_client {
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
                            self.logger.info(
                                "host_upload_refresh_retry: recovered upload with fresh client",
                            );
                            return Ok(self.upload_attempt_result(
                                &remote_file,
                                started.elapsed().as_millis() as u32,
                                true,
                                "",
                            ));
                        }
                        Err(refresh_err) => {
                            detail.push_str(&format!("; refresh_retry_err={}", refresh_err));
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
                    return Err(anyhow!("{detail}"));
                }

                if append_panic_signal_context(&mut detail, self.panic_signal()) {
                    return Err(anyhow!("{detail}"));
                }

                if let Some(class) = host_failure_class {
                    self.logger
                        .info(format!("HOST_FAILURE class={class} detail={detail}"));
                }

                Ok(self.upload_attempt_result(
                    &remote_file,
                    started.elapsed().as_millis() as u32,
                    false,
                    &detail,
                ))
            }
        }
    }

    pub(super) fn handle_net_verify_once(&mut self, context: &mut Value) -> Result<()> {
        let ip = ctx_get_string(context, "ip")?;
        let remote_file = ctx_get_string(context, "remote_file")?;
        let verify_timeout_sec = 30.0f64.max(0.5);
        let retry_policy = resolve_net_upload_retry_policy()?;
        if !upload::stat_remote_file(
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
}

#[cfg(test)]
#[path = "runtime_upload/tests.rs"]
mod tests;
