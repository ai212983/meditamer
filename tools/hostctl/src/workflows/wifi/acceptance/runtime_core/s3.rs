//! S3 console observations and commands; ordering belongs to the acceptance YAML.
use std::time::{Duration, Instant};

use anyhow::{anyhow, ensure, Result};
use regex::Regex;

use super::{
    super::WifiAcceptanceRuntime,
    start::{metric_bool, metric_u32},
};
use crate::serial_console::AckStatus;
use crate::workflows::wifi::common::netcfg_set_payload;

impl WifiAcceptanceRuntime<'_> {
    fn s3_command(&mut self, command: &str, operation: &str, timeout_s: u64) -> Result<()> {
        let (status, line) =
            self.console
                .command_wait_ack(command, "NET", Duration::from_secs(timeout_s))?;
        ensure!(
            status == AckStatus::Ok
                && line.as_deref().is_some_and(|line| line
                    .split_whitespace()
                    .any(|token| token == format!("op={operation}"))),
            "S3 command acknowledgement failed for {operation}: {}",
            if operation == "config_set" {
                "configuration acknowledgement missing or rejected (payload redacted)"
            } else {
                line.as_deref().unwrap_or("missing acknowledgement")
            }
        );
        Ok(())
    }

    pub(super) fn handle_s3_network_health(&mut self) -> Result<()> {
        self.handle_s3_runtime_health()?;
        let status = crate::workflows::wifi::common::query_net_status(&mut self.console)?
            .ok_or_else(|| anyhow!("S3 health missing NET_STATUS"))?;
        ensure!(
            crate::workflows::wifi::common::is_ready(&status, true),
            "S3 health: network/link/listener no longer ready"
        );
        Ok(())
    }

    pub(super) fn handle_s3_runtime_health(&mut self) -> Result<()> {
        let mark = self.console.mark();
        let line = self
            .console
            .wait_for_regex_since(
                mark,
                &Regex::new(r"^MEDINOTE_RUNTIME_METRICS ")?,
                Duration::from_secs(35),
            )?
            .ok_or_else(|| anyhow!("S3 UI health: fresh runtime metrics missing"))?;
        let uptime = validate_runtime_metrics(&line)?;
        if let Some(previous) = self.s3_uptime_ms {
            ensure!(
                uptime > previous,
                "S3 uptime did not advance: reboot or stale telemetry"
            );
        }
        self.s3_uptime_ms = Some(uptime);
        self.logger
            .info(format!("S3 runtime health gate: pass {line}"));
        Ok(())
    }

    pub(super) fn handle_s3_stop(&mut self) -> Result<()> {
        let mark = self.console.mark();
        self.s3_command("NET STOP", "stop", 35)?;
        // STOP acknowledgement follows set_enabled(false)'s physical quiescence.
        let line = crate::workflows::wifi::common::query_net_status_line(&mut self.console)?
            .ok_or_else(|| anyhow!("S3 stop: missing NET_STATUS"))?;
        let status: serde_json::Value = serde_json::from_str(
            line.strip_prefix("NET_STATUS ")
                .ok_or_else(|| anyhow!("invalid NET_STATUS"))?,
        )?;
        ensure!(
            self.console
                .read_recent_lines(mark)
                .iter()
                .any(|line| line
                    .starts_with("NET_POLICY enabled=false accepted=true radio_off=true")),
            "S3 stop missing physical radio-off acknowledgement"
        );
        ensure!(
            status["link"] == false && status["listener"] == false,
            "S3 stop did not quiesce radio/link/listener: {line}"
        );
        self.discovery_mark = self.console.mark();
        Ok(())
    }

    pub(super) fn handle_s3_configure(&mut self) -> Result<()> {
        let payload = netcfg_set_payload(&self.ssid, &self.password, self.policy);
        self.s3_command(&format!("NETCFG SET {payload}"), "config_set", 12)
    }

    pub(super) fn handle_s3_start(&mut self) -> Result<()> {
        self.s3_command("NET LISTENER ON", "listener_on", 4)?;
        let started = Instant::now();
        self.s3_command("NET START", "start", 110)?;
        self.s3_start_ack_samples
            .push(started.elapsed().as_secs_f64());
        Ok(())
    }

    pub(super) fn handle_s3_assert_discovery(&mut self) -> Result<()> {
        ensure!(self.console.read_recent_lines(self.discovery_mark).iter().any(|line|
            has_target_discovery(line)), "S3 discovery gate: no successful nonempty scan with target SSID evidence in this cycle");
        self.logger
            .info("S3 discovery gate: pass nonempty scan with target SSID");
        Ok(())
    }
}

fn has_target_discovery(line: &str) -> bool {
    line.starts_with("upload_http: scan_stage end ")
        && line.split_whitespace().any(|token| token == "outcome=ok")
        && metric_u32(line, "result_count").is_some_and(|count| count > 0)
        && metric_bool(line, "saw_target_after") == Some(true)
}

fn validate_runtime_metrics(line: &str) -> Result<u32> {
    let value =
        |key| metric_u32(line, key).ok_or_else(|| anyhow!("S3 metrics missing {key}: {line}"));
    ensure!(
        metric_bool(line, "lvgl_integrity_ok") == Some(true),
        "S3 LVGL integrity failed"
    );
    ensure!(
        metric_bool(line, "internal_heap_enabled") == Some(true),
        "S3 allocator metrics unavailable"
    );
    let total = value("lvgl_total")?;
    let free = value("lvgl_free")?;
    ensure!(
        total > 0 && value("lvgl_used")?.checked_add(free) == Some(total),
        "S3 LVGL accounting inconsistent"
    );
    ensure!(
        free >= 4096 && value("lvgl_largest_free")? >= 4096,
        "S3 LVGL free block below 4 KiB"
    );
    ensure!(
        value("internal_heap_free")? >= 16 * 1024,
        "S3 internal heap below 16 KiB reserve"
    );
    ensure!(
        value("internal_heap_max")? >= value("internal_heap_current")?,
        "S3 heap high water inconsistent"
    );
    value("uptime_ms")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_requires_success_nonempty_and_target() {
        let line = "upload_http: scan_stage end outcome=ok result_count=12 saw_target_after=true";
        assert!(has_target_discovery(line));
        for bad in [
            line.replace("ok", "error"),
            line.replace("12", "0"),
            line.replace("true", "false"),
        ] {
            assert!(!has_target_discovery(&bad));
        }
    }
    #[test]
    fn health_rejects_missing_and_unhealthy_metrics() {
        let line = "MEDINOTE_RUNTIME_METRICS uptime_ms=123 lvgl_total=20000 lvgl_used=10000 lvgl_free=10000 lvgl_largest_free=9000 lvgl_integrity_ok=true internal_heap_enabled=true internal_heap_free=17000 internal_heap_max=30000 internal_heap_current=29000";
        assert_eq!(validate_runtime_metrics(line).unwrap(), 123);
        for bad in [
            line.replace("17000", "16000"),
            line.replace("true", "false"),
            line.replace("9000", "100"),
            line.replace("uptime_ms=123", ""),
            line.replace("lvgl_total=20000", "lvgl_total=1"),
        ] {
            assert!(validate_runtime_metrics(&bad).is_err());
        }
    }
}
