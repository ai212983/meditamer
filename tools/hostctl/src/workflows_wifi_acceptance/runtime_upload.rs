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
        ctx_get_string, ctx_get_u32, ctx_set_bool, ctx_set_string, ctx_set_u32, wait_net_ack,
    },
};

use super::WifiAcceptanceRuntime;

impl WifiAcceptanceRuntime<'_> {
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
        let result = workflows_upload::upload_file_direct_fast(
            self.logger,
            &ip,
            8080,
            upload_timeout_sec,
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

    pub(super) fn handle_net_verify_once(&mut self, context: &mut Value) -> Result<()> {
        let remote_file = ctx_get_string(context, "remote_file")?;
        if !verify_remote_file(&mut self.console, &remote_file)? {
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
        if let Err(err) = wait_net_ack(&mut self.console, "NET RECOVER") {
            self.logger.info(format!(
                "net_recover_once: recover ack not obtained ({err}); continuing"
            ));
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

fn verify_remote_file(
    console: &mut crate::serial_console::SerialConsole,
    remote_path: &str,
) -> Result<bool> {
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
        let done = console.sdwait_for_id(req_id, 30_000)?.unwrap_or_default();
        if done.contains("SDWAIT DONE") && done.contains("status=ok") && done.contains("code=ok") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn avg(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}
