//! Exclusive Wi-Fi/BLE radio-handoff feasibility evidence.

mod allocator_status;
mod protocol;
mod setup;
#[cfg(test)]
mod tests;
mod validation;
mod workflow_runtime;

use allocator_status::collect_stack_metrics;
#[cfg(test)]
use allocator_status::{
    allocator_status_regex, parse_allocator_minimum, validate_allocator_minimum,
};
use protocol::*;
#[cfg(test)]
use setup::{build_phase1s_netcfg_command, validate_gate_options};
use validation::*;

use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow},
    serial_console::SerialConsole,
    workflows::{
        ble_phase1d::{validate_artifacts, wait_network_ready, ArtifactIdentity},
        common::repo_root,
        upload::{
            make_direct_upload_client, upload_file_direct_fast_with_client, DirectUploadOptions,
            UploadRetryPolicy,
        },
        wifi::{
            acceptance::ensure_host_wifi_association,
            common::{
                acquire_port_lock, detect_panic_signal, enforce_policy_floors, is_ready,
                query_net_status, NetPolicy, PortRunLock,
            },
        },
    },
};

const REQUIRED_OFF_FREE: u32 = 20_496;
const REQUIRED_CONTIGUOUS: u32 = 4_112;
const MAX_POST_WARMUP_DRIFT: u32 = 1_024;
const REQUIRED_GATE_CYCLES: u32 = 20;
const REQUIRED_CPU0_STACK_HEADROOM: u32 = 8 * 1_024;
const REQUIRED_TOUCH_STACK_HEADROOM: u32 = 1_024;
const REQUIRED_BLE_ACTIVE_FREE: u32 = 16_384;
const POST_RESTORE_HEALTH_TIMEOUT: Duration = Duration::from_secs(20);
const POST_RESTORE_HEALTH_POLL: Duration = Duration::from_millis(500);
const BLE_WINDOW_TIMEOUT: Duration = Duration::from_secs(6);
const NETCFG_TX_CHUNK_BYTES: usize = 32;
const NETCFG_TX_SETTLE_MS: u64 = 60;
const NETCFG_TX_SETTLE: Duration = Duration::from_millis(NETCFG_TX_SETTLE_MS);
const NETCFG_SAFE_LINE_BYTES: usize = 96;
const NETCFG_SSID_MAX_BYTES: usize = 32;
const NETCFG_PASSWORD_MAX_BYTES: usize = 64;
const _: () = assert!(NETCFG_TX_CHUNK_BYTES <= 32);
const _: () = assert!(NETCFG_TX_SETTLE_MS >= 60);
const _: () = assert!(NETCFG_TX_SETTLE_MS > 55);
const _: () = assert!(3 * NETCFG_TX_CHUNK_BYTES < 128);

#[derive(Clone, Debug)]
pub struct BlePhase1sOptions {
    pub artifacts: PathBuf,
    pub board_id: String,
    pub cycles: u32,
    pub output_path: Option<PathBuf>,
}

struct BlePhase1sRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    _port_lock: PortRunLock,
    identity: ArtifactIdentity,
    board_id: String,
    port: String,
    cycles: u32,
    ssid: String,
    policy: NetPolicy,
    netcfg_command: String,
    payload_path: PathBuf,
    report_path: PathBuf,
    evidence_mark: usize,
    boot_generation: Option<u32>,
    cpu0_stack_headroom_min: Option<u32>,
    touch_stack_headroom_min: Option<u32>,
    serving_internal_free_min: Option<u32>,
    serving_internal_min_alloc_charge: Option<u32>,
    serving_internal_min_alloc_internal_required: Option<bool>,
    serving_internal_min_alloc_wifi_rx_matched: Option<bool>,
    serving_internal_min_alloc_correlation_stable: Option<bool>,
    serving_internal_min_alloc_released: Option<bool>,
    uart_log_drops_baseline: Option<u32>,
    uart_log_drops_final: Option<u32>,
    samples: Vec<OffSample>,
    ble_samples: Vec<BleSample>,
    pending_cycle: Option<PendingCycle>,
    known_serving_rejection: bool,
    failure_stage: Option<String>,
    failure_reason: Option<String>,
    ownership_known: Option<bool>,
}

impl BlePhase1sRuntime<'_> {
    fn await_phase1s_identity(&mut self) -> Result<()> {
        let pong = Regex::new(r"^PONG$")?;
        let status_re = ble_window_status_regex()?;
        for _ in 0..40 {
            if self
                .console
                .command_wait_regex("PING", &pong, Duration::from_secs(2))?
                .is_some()
            {
                if let Some(line) = self.console.command_wait_regex(
                    "BLEP1S STATUS",
                    &status_re,
                    Duration::from_secs(2),
                )? {
                    let status = parse_ble_window_status(&line, &status_re)?;
                    if status.build_id != self.identity.build_id {
                        bail!("running BLE identity does not match the exact artifact: {line}");
                    }
                    return match status.state.as_str() {
                        "idle" | "completed" | "failed" => Ok(()),
                        "ownership_unknown" => {
                            Err(anyhow!("BLE ownership is unknown; reboot is required"))
                        }
                        state => Err(anyhow!("BLE Phase 1S window is already {state}")),
                    };
                }
            }
            thread::sleep(Duration::from_millis(250));
        }
        Err(anyhow!(
            "running device did not expose the expected Phase 1S BLE artifact identity"
        ))
    }

    fn await_sd_ready(&mut self) -> Result<()> {
        let line = self
            .console
            .sdwait_for_id(0, 15_000)?
            .ok_or_else(|| anyhow!("startup SD probe timed out without a correlated result"))?;
        let ready =
            Regex::new(r"^SDWAIT DONE target=id wait_id=0 id=0 op=probe status=ok code=ok(?: |$)")?;
        if !ready.is_match(&line) {
            bail!("startup SD probe did not reach ready: {line}");
        }
        self.reject_reset_or_panic_since_evidence_mark("startup SD readiness")?;
        Ok(())
    }

    fn apply_network_config_once(&mut self) -> Result<()> {
        let mark = self.console.mark();
        self.console.send_line_paced(
            &self.netcfg_command,
            NETCFG_TX_CHUNK_BYTES,
            NETCFG_TX_SETTLE,
        )?;
        let (status, line) = self
            .console
            .wait_ack_since(mark, "NET", Duration::from_secs(12))?;
        match (status, line.as_deref()) {
            (crate::serial_console::AckStatus::Ok, Some(line))
                if line.contains("op=config_set") =>
            {
                Ok(())
            }
            (crate::serial_console::AckStatus::Err, Some(line)) => {
                Err(anyhow!("NETCFG SET failed: {line}"))
            }
            _ => Err(anyhow!(
                "NETCFG SET had no unambiguous acknowledgement; command was not retried"
            )),
        }
    }

    fn wait_network_state(&mut self, require_listener: bool, timeout: Duration) -> Result<String> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if let Some(status) = query_net_status(&mut self.console)? {
                if is_ready(&status, require_listener) {
                    return status
                        .ipv4
                        .ok_or_else(|| anyhow!("ready NET_STATUS omitted IPv4"));
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
        Err(anyhow!(
            "network did not reach the required post-reset ready state"
        ))
    }

    fn wait_network_idle(&mut self) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            if query_net_status(&mut self.console)?.is_some_and(|status| {
                status.state.as_deref() == Some("Idle") && !status.link.unwrap_or(false)
            }) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(250));
        }
        Err(anyhow!("network did not confirm Idle after NET STOP"))
    }

    fn verify_applied_network_config(&mut self) -> Result<()> {
        let regex = Regex::new(r"^NETCFG \{.*\}$")?;
        let line = self
            .console
            .command_wait_regex("NETCFG GET", &regex, Duration::from_secs(3))?
            .ok_or_else(|| anyhow!("NETCFG GET timed out"))?;
        let value: Value = serde_json::from_str(
            line.strip_prefix("NETCFG ")
                .ok_or_else(|| anyhow!("invalid NETCFG GET response"))?,
        )?;
        if value.get("ssid_set") != Some(&Value::Bool(true))
            || value.get("ssid").and_then(Value::as_str) != Some(self.ssid.as_str())
            || value.get("policy") != Some(&serde_json::to_value(self.policy)?)
        {
            bail!("NETCFG GET did not match the requested SSID and policy");
        }
        Ok(())
    }

    fn capture_initial_owner_identity(&mut self) -> Result<()> {
        self.evidence_mark = self.console.mark();
        let (_, _, uart_log_drops) = self.query_stack_status()?;
        self.reject_reset_or_panic_since_evidence_mark("UART drop baseline")?;
        self.uart_log_drops_baseline = Some(uart_log_drops);
        self.evidence_mark = self.console.mark();
        let status = self.command_ack("RADIOHANDOFF STATUS", Duration::from_secs(3))?;
        if status.kind != "status" || !matches!(status.state.as_str(), "restoring" | "serving") {
            bail!("network owner is not safe to provision: {status:?}");
        }
        self.boot_generation = Some(status.boot);
        Ok(())
    }

    fn reject_reset_or_panic_since_evidence_mark(&mut self, stage: &str) -> Result<()> {
        for (line_index, line) in self
            .console
            .read_recent_lines(self.evidence_mark)
            .iter()
            .enumerate()
        {
            if let Some(signal) = detect_panic_signal(line, line_index) {
                bail!("{stage}: panic/reset signal detected: {signal:?}");
            }
            if line.contains("SERIAL_RX status=error") {
                bail!("{stage}: UART receive error detected: {line}");
            }
        }
        Ok(())
    }

    fn verify_post_reset_provisioning(&mut self) -> Result<()> {
        let status = self.command_ack("RADIOHANDOFF STATUS", Duration::from_secs(3))?;
        let boot = self
            .boot_generation
            .ok_or_else(|| anyhow!("missing initial boot generation"))?;
        validate_serving_health(0, boot, &status)?;
        self.reject_reset_or_panic_since_evidence_mark("post-reset provisioning")?;
        Ok(())
    }

    fn command_ack(&mut self, command: &str, timeout: Duration) -> Result<HandoffAck> {
        let regex = ack_regex()?;
        let line = self
            .console
            .command_wait_regex(command, &regex, timeout)?
            .ok_or_else(|| anyhow!("{command} timed out without retry"))?;
        parse_ack(&line, &regex)
    }

    fn upload_probe(&mut self, ip: &str, stage: &str, cycle: u32) -> Result<()> {
        let client = make_direct_upload_client(15.0)?;
        upload_file_direct_fast_with_client(
            self.logger,
            &client,
            DirectUploadOptions {
                host: ip,
                port: 8080,
                timeout_sec: 15.0,
                src: &self.payload_path,
                dst_root: "/assets/phase1s",
                token: None,
                retry_policy: UploadRetryPolicy {
                    sd_busy_total_retry_sec: 20.0,
                    net_recovery_timeout_sec: 20.0,
                    net_recovery_poll_sec: 0.5,
                    net_recovery_consecutive_health_successes: 1,
                },
            },
        )?;
        self.logger.info(format!(
            "Phase 1S cycle={cycle} stage={stage} upload passed"
        ));
        Ok(())
    }

    fn wait_post_restore_http_ready(&mut self, ip: &str) -> Result<()> {
        let client = make_direct_upload_client(3.0)?;
        let url = format!("http://{ip}:8080/health");
        let attempts = wait_for_post_restore_convergence(
            POST_RESTORE_HEALTH_TIMEOUT,
            POST_RESTORE_HEALTH_POLL,
            || match client.get(&url).send() {
                Ok(response) if response.status().is_success() => Ok(()),
                Ok(response) => Err(format!("GET {url} returned {}", response.status())),
                Err(error) => Err(format!("GET {url} send failed: {error}")),
            },
        )?;
        self.logger.info(format!(
            "Phase 1S post-restore HTTP convergence passed: attempts={attempts}"
        ));
        Ok(())
    }

    fn prepare_off_window(&mut self, cycle: u32) -> Result<()> {
        if self.pending_cycle.is_some() {
            bail!("cycle {cycle}: previous handoff cycle is still pending");
        }
        self.known_serving_rejection = false;
        let ip = wait_network_ready(&mut self.console)?;
        self.upload_probe(&ip, "before", cycle)?;

        let status = self.command_ack("RADIOHANDOFF STATUS", Duration::from_secs(3))?;
        if status.kind != "status" || status.state != "serving" {
            bail!("cycle {cycle}: owner is not serving: {status:?}");
        }
        let boot = *self.boot_generation.get_or_insert(status.boot);
        if status.boot != boot {
            bail!("cycle {cycle}: boot generation changed (reset detected)");
        }

        let off = self.command_ack(
            &format!("RADIOHANDOFF ACQUIRE {boot} {cycle}"),
            Duration::from_secs(30),
        )?;
        self.known_serving_rejection = is_known_serving_rejection(cycle, boot, &off);
        if off.kind == "quiesced"
            && off.state == "off_confirmed"
            && off.boot == boot
            && off.epoch == cycle
        {
            // Retain the first exact ownership proof before any resource or
            // settled-status check that can fail. This is the authority for a
            // later controlled rollback and the failure report.
            self.pending_cycle = Some(PendingCycle {
                cycle,
                boot,
                off: off.clone(),
                ble: None,
                ble_status: None,
            });
        }
        validate_off_ack(cycle, boot, &off)?;
        thread::sleep(Duration::from_millis(100));
        let settled = self.command_ack("RADIOHANDOFF STATUS", Duration::from_secs(3))?;
        validate_off_status(cycle, boot, &settled)?;
        self.pending_cycle
            .as_mut()
            .expect("exact quiesced acknowledgement installed the pending cycle")
            .off = settled;
        Ok(())
    }

    fn prepare_off_window_outcome(&mut self, cycle: u32) -> Value {
        match self.prepare_off_window(cycle) {
            Ok(()) => json!({ "handoff_outcome": "pass", "handoff_reason": "none" }),
            Err(error) if self.pending_cycle.is_some() => {
                self.failure_stage = Some("off_settlement".to_owned());
                self.failure_reason = Some(error.to_string());
                self.ownership_known = Some(true);
                json!({
                    "handoff_outcome": "known_off_failed",
                    "handoff_reason": error.to_string(),
                })
            }
            Err(error) if self.known_serving_rejection => {
                // The correlated Rejected/Serving terminal ACK proves rollback
                // to the Wi-Fi owner. Do not send Release again, but retain the
                // failure as known ownership rather than ambiguity.
                self.failure_stage = Some("handoff_acquire".to_owned());
                self.failure_reason = Some(error.to_string());
                self.ownership_known = Some(true);
                json!({
                    "handoff_outcome": "known_serving_failed",
                    "handoff_reason": error.to_string(),
                })
            }
            Err(error) => {
                self.failure_stage = Some("handoff_acquire".to_owned());
                self.failure_reason = Some(error.to_string());
                self.ownership_known = Some(false);
                json!({
                    "handoff_outcome": "ownership_unknown",
                    "handoff_reason": error.to_string(),
                })
            }
        }
    }

    fn ble_outcome(&mut self, outcome: &str, reason: impl Into<String>, known: bool) -> Value {
        let reason = reason.into();
        self.failure_stage = (outcome != "pass").then(|| "ble_window".to_owned());
        self.failure_reason = (outcome != "pass").then(|| reason.clone());
        self.ownership_known = Some(known);
        json!({
            "ble_outcome": outcome,
            "ble_reason": reason,
            "ble_ownership_known": known,
        })
    }

    fn run_ble_window(&mut self) -> Result<Value> {
        let pending = self
            .pending_cycle
            .as_ref()
            .ok_or_else(|| anyhow!("BLE window has no matching off-state lease"))?;
        let cycle = pending.cycle;
        let boot = pending.boot;
        let ack_re = ble_window_ack_regex()?;
        let command = format!("BLEP1S START {boot} {cycle}");
        let ack_line =
            match self
                .console
                .command_wait_regex(&command, &ack_re, Duration::from_secs(3))
            {
                Ok(Some(line)) => line,
                Ok(None) => {
                    return Ok(self.ble_outcome(
                        "ownership_unknown",
                        format!("cycle {cycle}: BLE start acknowledgement was ambiguous"),
                        false,
                    ));
                }
                Err(error) => {
                    return Ok(self.ble_outcome(
                        "ownership_unknown",
                        format!("cycle {cycle}: BLE start transport failed: {error}"),
                        false,
                    ));
                }
            };
        let Some(captures) = ack_re.captures(&ack_line) else {
            return Ok(self.ble_outcome(
                "ownership_unknown",
                format!("cycle {cycle}: invalid BLE start acknowledgement"),
                false,
            ));
        };
        let (ack_boot, ack_epoch) = match (captures[3].parse::<u32>(), captures[4].parse::<u32>()) {
            (Ok(boot), Ok(epoch)) => (boot, epoch),
            _ => {
                return Ok(self.ble_outcome(
                    "ownership_unknown",
                    format!("cycle {cycle}: BLE start acknowledgement identity overflowed"),
                    false,
                ));
            }
        };
        if ack_boot != boot || ack_epoch != cycle {
            return Ok(self.ble_outcome(
                "ownership_unknown",
                format!("cycle {cycle}: BLE start acknowledgement has stale lease identity"),
                false,
            ));
        }
        if &captures[1] != "queued" {
            let reason = captures[2].to_owned();
            let (outcome, ownership_known) = rejected_ble_outcome(&reason);
            return Ok(self.ble_outcome(outcome, reason, ownership_known));
        }

        let deadline = std::time::Instant::now() + BLE_WINDOW_TIMEOUT;
        let status_re = ble_window_status_regex()?;
        let terminal = loop {
            let line = match self.console.command_wait_regex(
                "BLEP1S STATUS",
                &status_re,
                Duration::from_secs(2),
            ) {
                Ok(Some(line)) => line,
                Ok(None) => {
                    return Ok(self.ble_outcome(
                        "ownership_unknown",
                        format!("cycle {cycle}: BLE status timed out without retry"),
                        false,
                    ));
                }
                Err(error) => {
                    return Ok(self.ble_outcome(
                        "ownership_unknown",
                        format!("cycle {cycle}: BLE status transport failed: {error}"),
                        false,
                    ));
                }
            };
            let status = match parse_ble_window_status(&line, &status_re) {
                Ok(status) => status,
                Err(error) => {
                    return Ok(self.ble_outcome(
                        "ownership_unknown",
                        format!("cycle {cycle}: invalid BLE status: {error}"),
                        false,
                    ));
                }
            };
            if status.build_id != self.identity.build_id
                || status.boot != boot
                || status.epoch != cycle
            {
                return Ok(self.ble_outcome(
                    "ownership_unknown",
                    format!("cycle {cycle}: BLE status has stale artifact or lease identity"),
                    false,
                ));
            }
            match status.state.as_str() {
                "completed" | "failed" | "ownership_unknown" => break status,
                "queued" | "running" if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(50));
                }
                "queued" | "running" => {
                    return Ok(self.ble_outcome(
                        "ownership_unknown",
                        format!("cycle {cycle}: BLE window did not close before its deadline"),
                        false,
                    ));
                }
                other => {
                    return Ok(self.ble_outcome(
                        "ownership_unknown",
                        format!("cycle {cycle}: unexpected BLE window state {other}"),
                        false,
                    ));
                }
            }
        };

        self.pending_cycle
            .as_mut()
            .expect("pending cycle exists while BLE status is retained")
            .ble_status = Some(terminal.clone());

        if terminal.state == "ownership_unknown" {
            return Ok(self.ble_outcome("ownership_unknown", terminal.failure, false));
        }
        if terminal.state == "failed" {
            return Ok(self.ble_outcome("known_failed", terminal.failure, true));
        }

        let sample = match validate_ble_window(cycle, boot, &terminal) {
            Ok(sample) => sample,
            Err(error) => return Ok(self.ble_outcome("known_failed", error.to_string(), true)),
        };
        self.pending_cycle
            .as_mut()
            .expect("pending cycle exists while BLE window is validated")
            .ble = Some(sample);
        Ok(self.ble_outcome("pass", "none", true))
    }

    fn restore_after_known_failure(&mut self) -> Result<()> {
        if let Err(error) = self.restore_pending_cycle(false) {
            let mut reason = error.to_string();
            let ownership_known = self.revalidate_restored_ownership(&mut reason);
            self.failure_stage = Some(restore_failure_stage(ownership_known).to_owned());
            let disposition = if ownership_known {
                "Wi-Fi ownership was restored but service readiness failed"
            } else {
                "Wi-Fi restoration was ambiguous"
            };
            let reason = format!(
                "{}; {disposition}: {reason}",
                self.failure_reason
                    .as_deref()
                    .unwrap_or("BLE window failed")
            );
            self.failure_reason = Some(reason);
        }
        Ok(())
    }

    fn restore_pending_cycle(&mut self, commit: bool) -> Result<()> {
        let pending = self
            .pending_cycle
            .as_ref()
            .ok_or_else(|| anyhow!("no pending exclusive lease to restore"))?;
        let cycle = pending.cycle;
        let boot = pending.boot;
        // Until the exact matching Restored acknowledgement arrives, ownership
        // is ambiguous. Service failures after that point must not erase the
        // proof that Wi-Fi owns the radio again.
        self.ownership_known = Some(false);
        let restored = self.command_ack(
            &format!("RADIOHANDOFF RELEASE {boot} {cycle}"),
            Duration::from_secs(190),
        )?;
        self.accept_restored_ownership(cycle, boot, &restored)?;
        if !commit {
            let _ = wait_network_ready(&mut self.console)?;
            return Ok(());
        }
        let ip = wait_network_ready(&mut self.console)?;
        self.wait_post_restore_http_ready(&ip)?;
        self.upload_probe(&ip, "after", cycle)?;
        let post_restore = self.command_ack("RADIOHANDOFF STATUS", Duration::from_secs(3))?;
        validate_serving_health(cycle, boot, &post_restore)?;
        for (line_index, line) in self
            .console
            .read_recent_lines(self.evidence_mark)
            .iter()
            .enumerate()
        {
            if let Some(signal) = detect_panic_signal(line, line_index) {
                bail!("cycle {cycle}: panic/reset signal detected: {signal:?}");
            }
            if line.contains("SERIAL_RX status=error") {
                bail!("cycle {cycle}: UART receive error detected: {line}");
            }
        }
        let pending = self
            .pending_cycle
            .take()
            .expect("pending success evidence remains until restoration succeeds");
        let ble = pending
            .ble
            .ok_or_else(|| anyhow!("cycle {cycle}: restoration attempted without BLE evidence"))?;
        self.samples.push(OffSample {
            cycle,
            boot,
            epoch: pending.off.epoch,
            internal_free: pending.off.internal_free,
            largest_block: pending.off.largest_block,
            probe_free_before: pending.off.probe_free_before,
            probe_free_after: pending.off.probe_free_after,
            probe_reserve: pending.off.probe_reserve,
            late_callbacks: pending.off.late_callbacks,
            queue_late_use: pending.off.queue_late_use,
            queue_unknown_use: pending.off.queue_unknown_use,
            queue_reclaim_failures: pending.off.queue_reclaim_failures,
            queue_corruption: pending.off.queue_corruption,
            queue_contention: pending.off.queue_contention,
            post_restore_late_callbacks: post_restore.late_callbacks,
            post_restore_queue_late_use: post_restore.queue_late_use,
            post_restore_queue_unknown_use: post_restore.queue_unknown_use,
            post_restore_queue_reclaim_failures: post_restore.queue_reclaim_failures,
            post_restore_queue_corruption: post_restore.queue_corruption,
            post_restore_queue_contention: post_restore.queue_contention,
        });
        self.ble_samples.push(ble);
        Ok(())
    }

    fn accept_restored_ownership(
        &mut self,
        cycle: u32,
        boot: u32,
        restored: &HandoffAck,
    ) -> Result<()> {
        if restored.kind != "restored"
            || restored.state != "serving"
            || restored.boot != boot
            || restored.epoch != cycle
        {
            bail!("cycle {cycle}: restoration failed: {restored:?}");
        }
        self.ownership_known = Some(true);
        Ok(())
    }

    fn restore_and_complete_cycle_outcome(&mut self) -> Value {
        match self.restore_pending_cycle(true) {
            Ok(()) => json!({ "restore_outcome": "pass", "restore_reason": "none" }),
            Err(error) => {
                let mut reason = error.to_string();
                let ownership_known = self.revalidate_restored_ownership(&mut reason);
                self.failure_stage = Some(restore_failure_stage(ownership_known).to_owned());
                self.failure_reason = Some(reason.clone());
                json!({
                    "restore_outcome": "failed",
                    "restore_reason": reason,
                })
            }
        }
    }

    fn revalidate_restored_ownership(&mut self, reason: &mut String) -> bool {
        if !self.ownership_known.unwrap_or(false) {
            return false;
        }
        let Some((cycle, boot)) = self
            .pending_cycle
            .as_ref()
            .map(|pending| (pending.cycle, pending.boot))
        else {
            self.ownership_known = Some(false);
            reason.push_str("; restored ownership could not be correlated to a pending lease");
            return false;
        };
        let verification = self
            .reject_reset_or_panic_since_evidence_mark("post-restore service failure")
            .and_then(|()| {
                let status = self.command_ack("RADIOHANDOFF STATUS", Duration::from_secs(3))?;
                validate_serving_health(cycle, boot, &status)?;
                self.reject_reset_or_panic_since_evidence_mark("post-restore ownership status")
            });
        match verification {
            Ok(()) => true,
            Err(error) => {
                self.ownership_known = Some(false);
                reason.push_str(&format!(
                    "; restored ownership revalidation failed: {error}"
                ));
                false
            }
        }
    }

    fn collect_stack_metrics_outcome(&mut self) -> Value {
        match collect_stack_metrics(self) {
            Ok(()) => json!({
                "metrics_outcome": "pass",
                "metrics_reason": "none",
            }),
            Err(error) => {
                let reason = error.to_string();
                self.failure_stage = Some("final_metrics".to_owned());
                self.failure_reason = Some(reason.clone());
                json!({
                    "metrics_outcome": "failed",
                    "metrics_reason": reason,
                })
            }
        }
    }

    fn query_stack_status(&mut self) -> Result<(u32, u32, u32)> {
        let status_re = stack_status_regex()?;
        let status_line = self
            .console
            .command_wait_regex("STACKSTATUS", &status_re, Duration::from_secs(5))?
            .ok_or_else(|| anyhow!("missing correlated stack status"))?;
        parse_stack_status(&status_line, &status_re)
    }

    fn assert_stack_floors(&mut self) -> Result<()> {
        let main = self
            .cpu0_stack_headroom_min
            .ok_or_else(|| anyhow!("CPU0 stack floor was not collected"))?;
        let touch = self
            .touch_stack_headroom_min
            .ok_or_else(|| anyhow!("touch-core stack floor was not collected"))?;
        let (free_drift, block_drift) = self.post_warmup_drifts();
        let violations = self.final_gate_violations(free_drift, block_drift)?;
        if !violations.is_empty() {
            bail!("Phase 1S final gate failed: {}", violations.join("; "));
        }
        self.logger.info(format!(
            "Phase 1S exclusive Wi-Fi/BLE gate passed: cycles={} cpu0_stack={} touch_stack={} uart_log_drops=0 report={}",
            self.samples.len(),
            main,
            touch,
            self.report_path.display()
        ));
        Ok(())
    }

    fn finish_report(&self) -> Result<Phase1sReport> {
        validate_completed_samples(&self.samples, self.cycles)?;
        validate_completed_ble_samples(&self.ble_samples, self.cycles)?;
        validate_correlated_samples(&self.samples, &self.ble_samples)?;
        let cpu0_stack_headroom_min = self
            .cpu0_stack_headroom_min
            .ok_or_else(|| anyhow!("CPU0 stack floor was not collected"))?;
        let touch_stack_headroom_min = self
            .touch_stack_headroom_min
            .ok_or_else(|| anyhow!("touch-core stack floor was not collected"))?;
        let uart_log_drops_baseline = self
            .uart_log_drops_baseline
            .ok_or_else(|| anyhow!("UART diagnostic drop baseline was not collected"))?;
        let uart_log_drops_final = self
            .uart_log_drops_final
            .ok_or_else(|| anyhow!("UART diagnostic final drop count was not collected"))?;
        let uart_log_drops_during_gate =
            validate_uart_drop_counter(uart_log_drops_baseline, uart_log_drops_final).ok();
        let (free_drift, block_drift) = self.post_warmup_drifts();
        let violations = self.final_gate_violations(free_drift, block_drift)?;
        let gate_passed = violations.is_empty();
        Ok(Phase1sReport {
            schema_version: 2,
            gate_kind: "phase1s_wifi_ble_exclusive",
            board_id: self.board_id.clone(),
            build_id: self.identity.build_id.clone(),
            source_git_head: self.identity.git_head.clone(),
            artifact_elf_sha256: self.identity.elf_sha256.clone(),
            artifact_app_sha256: self.identity.app_sha256.clone(),
            serial_port: self.port.clone(),
            network_ssid: self.ssid.clone(),
            completed_cycles: self.samples.len() as u32,
            completed_ble_cycles: self.ble_samples.len() as u32,
            first_ble_allocation_delta_bytes: self
                .ble_samples
                .first()
                .map(|sample| sample.allocation_delta),
            minimum_ble_active_internal_free_bytes: self
                .ble_samples
                .iter()
                .map(|sample| sample.active_free)
                .min(),
            minimum_serving_internal_free_bytes: self.serving_internal_free_min,
            minimum_serving_internal_alloc_charge_bytes: self.serving_internal_min_alloc_charge,
            minimum_serving_internal_alloc_internal_required: self
                .serving_internal_min_alloc_internal_required,
            minimum_serving_internal_alloc_wifi_rx_matched: self
                .serving_internal_min_alloc_wifi_rx_matched,
            minimum_serving_internal_alloc_correlation_stable: self
                .serving_internal_min_alloc_correlation_stable,
            minimum_serving_internal_alloc_released: self.serving_internal_min_alloc_released,
            post_warmup_free_drift: free_drift,
            post_warmup_largest_block_drift: block_drift,
            cpu0_stack_headroom_min: Some(cpu0_stack_headroom_min),
            touch_stack_headroom_min: Some(touch_stack_headroom_min),
            uart_log_drops_baseline: Some(uart_log_drops_baseline),
            uart_log_drops_final: Some(uart_log_drops_final),
            uart_log_drops_during_gate,
            failure_stage: None,
            failure_reason: None,
            ownership_known: Some(true),
            pending_off: None,
            pending_ble_status: None,
            gate_passed,
            violations,
            off_samples: self.samples.clone(),
            ble_samples: self.ble_samples.clone(),
        })
    }

    fn finish_failure_report(&self) -> Phase1sReport {
        let (free_drift, block_drift) = self.post_warmup_drifts();
        let uart_log_drops_during_gate = self
            .uart_log_drops_baseline
            .zip(self.uart_log_drops_final)
            .and_then(|(baseline, final_count)| {
                validate_uart_drop_counter(baseline, final_count).ok()
            });
        let reason = self
            .failure_reason
            .clone()
            .unwrap_or_else(|| "BLE lifecycle failed without a classified reason".to_owned());
        Phase1sReport {
            schema_version: 2,
            gate_kind: "phase1s_wifi_ble_exclusive",
            board_id: self.board_id.clone(),
            build_id: self.identity.build_id.clone(),
            source_git_head: self.identity.git_head.clone(),
            artifact_elf_sha256: self.identity.elf_sha256.clone(),
            artifact_app_sha256: self.identity.app_sha256.clone(),
            serial_port: self.port.clone(),
            network_ssid: self.ssid.clone(),
            completed_cycles: self.samples.len() as u32,
            completed_ble_cycles: self.ble_samples.len() as u32,
            first_ble_allocation_delta_bytes: self
                .ble_samples
                .first()
                .map(|sample| sample.allocation_delta),
            minimum_ble_active_internal_free_bytes: self
                .ble_samples
                .iter()
                .map(|sample| sample.active_free)
                .min(),
            minimum_serving_internal_free_bytes: self.serving_internal_free_min,
            minimum_serving_internal_alloc_charge_bytes: self.serving_internal_min_alloc_charge,
            minimum_serving_internal_alloc_internal_required: self
                .serving_internal_min_alloc_internal_required,
            minimum_serving_internal_alloc_wifi_rx_matched: self
                .serving_internal_min_alloc_wifi_rx_matched,
            minimum_serving_internal_alloc_correlation_stable: self
                .serving_internal_min_alloc_correlation_stable,
            minimum_serving_internal_alloc_released: self.serving_internal_min_alloc_released,
            post_warmup_free_drift: free_drift,
            post_warmup_largest_block_drift: block_drift,
            cpu0_stack_headroom_min: self.cpu0_stack_headroom_min,
            touch_stack_headroom_min: self.touch_stack_headroom_min,
            uart_log_drops_baseline: self.uart_log_drops_baseline,
            uart_log_drops_final: self.uart_log_drops_final,
            uart_log_drops_during_gate,
            failure_stage: self.failure_stage.clone(),
            failure_reason: Some(reason.clone()),
            ownership_known: self.ownership_known,
            pending_off: self
                .pending_cycle
                .as_ref()
                .map(|pending| pending.off.clone()),
            pending_ble_status: self
                .pending_cycle
                .as_ref()
                .and_then(|pending| pending.ble_status.clone()),
            gate_passed: false,
            violations: vec![reason],
            off_samples: self.samples.clone(),
            ble_samples: self.ble_samples.clone(),
        }
    }

    fn post_warmup_drifts(&self) -> (u32, u32) {
        let warm = self.samples.iter().skip(1);
        (
            drift(warm.clone().map(|sample| sample.internal_free)),
            drift(warm.map(|sample| sample.largest_block)),
        )
    }

    fn final_gate_violations(&self, free_drift: u32, block_drift: u32) -> Result<Vec<String>> {
        let main = self
            .cpu0_stack_headroom_min
            .ok_or_else(|| anyhow!("CPU0 stack floor was not collected"))?;
        let touch = self
            .touch_stack_headroom_min
            .ok_or_else(|| anyhow!("touch-core stack floor was not collected"))?;
        let baseline = self
            .uart_log_drops_baseline
            .ok_or_else(|| anyhow!("UART diagnostic drop baseline was not collected"))?;
        let final_count = self
            .uart_log_drops_final
            .ok_or_else(|| anyhow!("UART diagnostic final drop count was not collected"))?;
        let mut violations = Vec::new();
        if let Err(error) = validate_stack_floors(main, touch) {
            violations.push(error.to_string());
        }
        match self.serving_internal_free_min {
            Some(minimum) if minimum >= REQUIRED_BLE_ACTIVE_FREE => {}
            Some(minimum) => violations.push(format!(
                "serving internal-free floor failed: minimum={} required={}",
                minimum, REQUIRED_BLE_ACTIVE_FREE
            )),
            None => violations.push("serving internal-free floor was not collected".to_owned()),
        }
        match validate_uart_drop_counter(baseline, final_count) {
            Ok(0) => {}
            Ok(during_gate) => violations.push(format!(
                "UART diagnostic drop gate failed: during_gate={during_gate}"
            )),
            Err(error) => violations.push(error.to_string()),
        }
        if free_drift > MAX_POST_WARMUP_DRIFT || block_drift > MAX_POST_WARMUP_DRIFT {
            violations.push(format!(
                "post-warm-up drift exceeded {} bytes: free={} largest_block={}",
                MAX_POST_WARMUP_DRIFT, free_drift, block_drift
            ));
        }
        let ble_active_drift = drift(
            self.ble_samples
                .iter()
                .skip(1)
                .map(|sample| sample.active_free),
        );
        let ble_after_drift = drift(
            self.ble_samples
                .iter()
                .skip(1)
                .map(|sample| sample.after_free),
        );
        if ble_active_drift > MAX_POST_WARMUP_DRIFT || ble_after_drift > MAX_POST_WARMUP_DRIFT {
            violations.push(format!(
                "BLE post-warm-up drift exceeded {} bytes: active={} after={}",
                MAX_POST_WARMUP_DRIFT, ble_active_drift, ble_after_drift
            ));
        }
        Ok(violations)
    }
}

pub fn run_ble_phase1s(logger: &mut Logger, opts: BlePhase1sOptions) -> Result<()> {
    setup::run_ble_phase1s_inner(logger, opts)
}
