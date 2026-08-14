use std::{
    fs, thread,
    time::{Duration, Instant},
};

use super::helpers::{avg, parse_metrics_key_u32};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

use crate::{
    env_utils,
    workflows::wifi::common::{
        ctx_get_string, ctx_get_u32, is_ready, net_status_line_re, query_net_status,
    },
};

use super::super::WifiAcceptanceRuntime;

impl WifiAcceptanceRuntime<'_> {
    pub(in crate::workflows::wifi::acceptance) fn handle_net_collect_diag(&mut self) -> Result<()> {
        let status_re = net_status_line_re();
        let mark = self.console.mark();
        self.console.send_line("NET STATUS")?;
        if let Some(line) =
            self.console
                .wait_for_regex_since(mark, status_re, Duration::from_secs(2))?
        {
            self.logger.info(format!("diag: {line}"));
        }
        Ok(())
    }

    pub(in crate::workflows::wifi::acceptance) fn handle_net_recover_once(&mut self) -> Result<()> {
        if self.reuse_upload_client {
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

    pub(in crate::workflows::wifi::acceptance) fn handle_fail_upload(
        &mut self,
        context: &mut Value,
    ) -> Result<()> {
        self.log_mem_summary("failure summary");
        let detail = ctx_get_string(context, "upload_error")
            .unwrap_or_else(|_| "network/upload workflow failed".to_string());
        Err(anyhow!("{detail}"))
    }

    pub(in crate::workflows::wifi::acceptance) fn handle_finalize_cycle(
        &mut self,
        context: &mut Value,
    ) -> Result<()> {
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

    pub(in crate::workflows::wifi::acceptance) fn handle_print_summary(&mut self) -> Result<()> {
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

    pub(in crate::workflows::wifi::acceptance) fn query_req_read_body_reset(
        &mut self,
    ) -> Result<u32> {
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
