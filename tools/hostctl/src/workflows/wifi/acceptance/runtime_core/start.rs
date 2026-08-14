use std::{
    fs, thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

use crate::{env_utils, logging::ensure_parent_dir, serial_console::AckStatus};

use super::super::{
    boot_gate::{run_boot_discovery_gate, BootDiscoveryGateConfig},
    WifiAcceptanceRuntime,
};

impl WifiAcceptanceRuntime<'_> {
    pub(super) fn handle_wait_runtime_ready(&mut self) -> Result<()> {
        let ready_marker = Regex::new(r"^RUNTIME_READY app_state=ready display=ready$")?;
        let ready_status = Regex::new(r"^SCHEDPROFILE .*runtime_ready=on$")?;
        let timeout_ms = env_utils::parse_env_u32("HOSTCTL_NET_RUNTIME_READY_TIMEOUT_MS", 45_000)?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms.into());
        while Instant::now() < deadline {
            if self
                .console
                .find_first_regex_since(0, &ready_marker)
                .is_some()
                || self
                    .console
                    .command_wait_regex("SCHEDPROFILE", &ready_status, Duration::from_millis(750))?
                    .is_some()
            {
                self.logger.info("runtime readiness gate: pass");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(anyhow!(
            "runtime readiness not observed within {} ms",
            timeout_ms
        ))
    }

    pub(super) fn handle_boot_discovery_gate(&mut self) -> Result<()> {
        if !env_utils::parse_env_bool01("HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE", true)? {
            return Ok(());
        }
        let cfg = BootDiscoveryGateConfig {
            max_boot_uptime_ms: env_utils::parse_env_u32(
                "HOSTCTL_NET_BOOT_DISCOVERY_MAX_UPTIME_MS",
                30_000,
            )?,
            timeout_ms: env_utils::parse_env_u32("HOSTCTL_NET_BOOT_DISCOVERY_TIMEOUT_MS", 180_000)?,
            settle_ms: env_utils::parse_env_u32("HOSTCTL_NET_BOOT_DISCOVERY_SETTLE_MS", 6_000)?,
            allow_ready_only_fallback: env_utils::parse_env_bool01(
                "HOSTCTL_NET_BOOT_DISCOVERY_READY_ONLY_FALLBACK",
                false,
            )?,
        };
        run_boot_discovery_gate(
            self.logger,
            &mut self.console,
            &self.ssid,
            &self.password,
            self.policy,
            cfg,
        )
    }

    pub(super) fn handle_prepare_payload(&mut self) -> Result<()> {
        ensure_parent_dir(&self.payload_path)?;
        let mut data = vec![0u8; 524_288];
        for (i, slot) in data.iter_mut().enumerate() {
            *slot = ((i * 17 + 31) & 0xFF) as u8;
        }
        fs::write(&self.payload_path, data)?;
        Ok(())
    }

    pub(super) fn build_start_run_result(&self) -> Value {
        serde_json::json!({
            "cycle": 1,
            "cycles": self.cycles,
            "operation_retries": self.operation_retries
        })
    }

    pub(super) fn handle_start_run(&mut self) -> Result<()> {
        self.ensure_operating_upload_mode()?;
        self.mem_read_mark = self.console.mark();
        self.panic_monitoring_enabled = true;
        self.panic_first = None;
        self.req_read_body_reset_baseline = Some(self.query_req_read_body_reset()?);
        Ok(())
    }

    pub(super) fn handle_prepare_measurement(&mut self) -> Result<()> {
        let quiet = Regex::new(r"^TELEMSET OK mask=0x00 ")?;
        self.console
            .command_wait_regex("TELEMSET ALL OFF", &quiet, Duration::from_secs(3))?
            .ok_or_else(|| anyhow!("telemetry quiet-mode acknowledgement not observed"))?;

        let reset = Regex::new(r"^TOUCHSCHEDRESET OK$")?;
        self.console
            .command_wait_regex("TOUCHSCHEDRESET", &reset, Duration::from_secs(3))?
            .ok_or_else(|| anyhow!("touch scheduler reset acknowledgement not observed"))?;
        self.logger
            .info("measurement window: verbose telemetry off; touch scheduler reset");
        Ok(())
    }

    pub(super) fn handle_assert_runtime_health(&mut self) -> Result<()> {
        let touch_re = Regex::new(r"^METRICS TOUCH_SCHED ")?;
        let touch_line = self
            .console
            .command_wait_regex("METRICS", &touch_re, Duration::from_secs(5))?
            .ok_or_else(|| anyhow!("missing METRICS TOUCH_SCHED response"))?;
        let active_gap = metric_u32(&touch_line, "active_gap_max_ms")
            .ok_or_else(|| anyhow!("touch metrics missing active_gap_max_ms: {touch_line}"))?;
        let active_limit = env_utils::parse_env_u32("HOSTCTL_TOUCH_ACTIVE_GAP_MAX_MS", 16)?;
        if active_gap > active_limit {
            return Err(anyhow!(
                "touch scheduling gate failed: active_gap_max_ms={} limit={}",
                active_gap,
                active_limit
            ));
        }

        let main_stack_re = Regex::new(r"^stack_diag: tag=minimum headroom=")?;
        let main_stack_line = self
            .console
            .command_wait_regex("METRICS", &main_stack_re, Duration::from_secs(5))?
            .ok_or_else(|| anyhow!("missing main stack headroom response"))?;
        let main_stack_headroom = metric_u32(&main_stack_line, "headroom")
            .ok_or_else(|| anyhow!("main stack response missing headroom: {main_stack_line}"))?;
        let main_stack_floor =
            env_utils::parse_env_u32("HOSTCTL_MAIN_STACK_HEADROOM_MIN_BYTES", 8 * 1024)?;
        if main_stack_headroom < main_stack_floor {
            return Err(anyhow!(
                "main stack gate failed: headroom={} floor={}",
                main_stack_headroom,
                main_stack_floor
            ));
        }

        let touch_stack_re = Regex::new(r"^touch_core_stack_diag: tag=minimum headroom=")?;
        let touch_stack_line = self
            .console
            .command_wait_regex("METRICS", &touch_stack_re, Duration::from_secs(5))?
            .ok_or_else(|| anyhow!("missing touch-core stack headroom response"))?;
        let touch_stack_headroom = metric_u32(&touch_stack_line, "headroom").ok_or_else(|| {
            anyhow!("touch-core stack response missing headroom: {touch_stack_line}")
        })?;
        let touch_stack_floor =
            env_utils::parse_env_u32("HOSTCTL_TOUCH_CORE_STACK_HEADROOM_MIN_BYTES", 1024)?;
        if touch_stack_headroom < touch_stack_floor {
            return Err(anyhow!(
                "touch-core stack gate failed: headroom={} floor={}",
                touch_stack_headroom,
                touch_stack_floor
            ));
        }

        let memory_re = Regex::new(r"^PSRAM ")?;
        let memory_line = self
            .console
            .command_wait_regex("PSRAM", &memory_re, Duration::from_secs(5))?
            .ok_or_else(|| anyhow!("missing PSRAM allocator response"))?;
        let memory = parse_serving_allocator_status(&memory_line)?;
        let memory_floor =
            env_utils::parse_env_u32("HOSTCTL_NET_MIN_INTERNAL_FREE_BYTES", 16 * 1024)?;
        // Gate on the live, current internal-free reading, not `minimum_internal`
        // (`min_internal_free_bytes`): that field is a monotonic, boot-lifetime
        // low-water register that only ever ratchets downward and is never reset
        // except at boot (see `src/firmware/psram/` — `seed_internal_low_water`
        // runs once, inside `init_allocator`'s one-shot init guard). It records the
        // single worst instant since boot, not the current or at-time-of-need value,
        // so a long-running, teardown-free session (like this workflow's 3+ cycles
        // in one boot) trips it on pure order statistics with no leak involved — see
        // CAP-0013/CAP-0014 in docs/plans/ble-phase1s-capacity-recovery-ledger.md.
        // The product's own BLE-admission gate
        // (`settled_off_resource_snapshot`/`finish_product_quiescence` in
        // src/firmware/net/runtime.rs) never reads this register either — it
        // re-probes current free bytes at the moment of need, which is what this
        // check now mirrors. `minimum_internal` is retained below in the
        // informational log line for visibility, just not used as the pass/fail
        // criterion.
        if memory.current_internal < memory_floor {
            return Err(anyhow!(
                "internal memory gate failed: internal_free_bytes={} floor={} min_internal_free_bytes={} min_internal_alloc_charge_bytes={} min_internal_alloc_internal_required={} min_internal_alloc_charge_overflow={} min_internal_alloc_post_free_bytes={} min_internal_alloc_wifi_rx_matched={}",
                memory.current_internal,
                memory_floor,
                memory.minimum_internal,
                memory.minimum_alloc_charge,
                memory.minimum_alloc_internal_required,
                memory.minimum_alloc_charge_overflow,
                memory.minimum_alloc_post_free,
                memory.minimum_alloc_wifi_rx_matched,
            ));
        }

        self.logger.info(format!(
            "runtime_health_gate: active_gap_max_ms={} main_stack_headroom={} touch_core_stack_headroom={} internal_free_bytes={} min_internal_free_bytes={} min_internal_alloc_charge_bytes={} min_internal_alloc_internal_required={} min_internal_alloc_charge_overflow={} min_internal_alloc_post_free_bytes={} min_internal_alloc_wifi_rx_matched={} internal_probe_block_bytes={} internal_probe_reserve_bytes={} probe_stable={}",
            active_gap,
            main_stack_headroom,
            touch_stack_headroom,
            memory.current_internal,
            memory.minimum_internal,
            memory.minimum_alloc_charge,
            memory.minimum_alloc_internal_required,
            memory.minimum_alloc_charge_overflow,
            memory.minimum_alloc_post_free,
            memory.minimum_alloc_wifi_rx_matched,
            memory.probe_block,
            memory.probe_reserve,
            memory.probe_stable,
        ));
        Ok(())
    }

    fn ensure_operating_upload_mode(&mut self) -> Result<()> {
        if !env_utils::parse_env_bool01("HOSTCTL_NET_ENSURE_OPERATING_MODE", true)? {
            return Ok(());
        }
        self.send_state_command_with_ack("STATE SET upload=on")?;
        self.send_state_command_with_ack("STATE DIAG kind=NONE targets=NONE")?;
        let state_re = Regex::new(r"^STATE phase=").expect("state regex");
        let mark = self.console.mark();
        self.console.send_line("STATE GET")?;
        if let Some(line) =
            self.console
                .wait_for_regex_since(mark, &state_re, Duration::from_secs(4))?
        {
            self.logger.info(format!("net_start state_probe: {line}"));
        }
        self.console.settle(200)?;
        Ok(())
    }

    fn send_state_command_with_ack(&mut self, command: &str) -> Result<()> {
        const ATTEMPTS: usize = 4;
        for attempt in 1..=ATTEMPTS {
            let mark = self.console.mark();
            self.console.send_line(command)?;
            let (status, line) =
                self.console
                    .wait_ack_since(mark, "STATE", Duration::from_secs(4))?;
            match status {
                AckStatus::Ok => return Ok(()),
                AckStatus::Busy | AckStatus::None => {
                    if attempt < ATTEMPTS {
                        thread::sleep(Duration::from_millis(200));
                        continue;
                    }
                    return Err(anyhow!(
                        "state command did not ack after {} attempt(s): {}",
                        ATTEMPTS,
                        command
                    ));
                }
                AckStatus::Err => {
                    return Err(anyhow!(
                        "state command failed: {} ({})",
                        command,
                        line.unwrap_or_else(|| "STATE ERR".to_string())
                    ));
                }
            }
        }
        Err(anyhow!("unreachable state-ack retry path"))
    }

    pub(super) fn build_init_upload_attempt_result(&self) -> Value {
        serde_json::json!({
            "upload_attempt": 1,
            "upload_done": false
        })
    }

    pub(super) fn handle_init_upload_attempt(&self) -> Result<()> {
        Ok(())
    }
}

pub(super) struct ServingAllocatorStatus {
    current_internal: u32,
    minimum_internal: u32,
    minimum_alloc_charge: u32,
    minimum_alloc_internal_required: bool,
    minimum_alloc_charge_overflow: bool,
    minimum_alloc_post_free: u32,
    minimum_alloc_correlation_stable: bool,
    minimum_alloc_wifi_rx_matched: bool,
    minimum_alloc_released: bool,
    probe_performed: bool,
    probe_block: u32,
    probe_reserve: u32,
    probe_before: u32,
    probe_after: u32,
    probe_stable: bool,
}

pub(super) fn parse_serving_allocator_status(line: &str) -> Result<ServingAllocatorStatus> {
    let required_u32 =
        |key| metric_u32(line, key).ok_or_else(|| anyhow!("PSRAM status missing {key}: {line}"));
    let current_internal = required_u32("internal_free_bytes")?;
    let minimum_internal = required_u32("min_internal_free_bytes")?;
    let minimum_alloc_charge = required_u32("min_internal_alloc_charge_bytes")?;
    let minimum_alloc_internal_required = metric_bool(line, "min_internal_alloc_internal_required")
        .ok_or_else(|| {
            anyhow!("PSRAM status missing min_internal_alloc_internal_required: {line}")
        })?;
    let minimum_alloc_charge_overflow = metric_bool(line, "min_internal_alloc_charge_overflow")
        .ok_or_else(|| {
            anyhow!("PSRAM status missing min_internal_alloc_charge_overflow: {line}")
        })?;
    let minimum_alloc_post_free = required_u32("min_internal_alloc_post_free_bytes")?;
    let minimum_alloc_correlation_stable =
        metric_bool(line, "min_internal_alloc_correlation_stable").ok_or_else(|| {
            anyhow!("PSRAM status missing min_internal_alloc_correlation_stable: {line}")
        })?;
    let minimum_alloc_wifi_rx_matched = metric_bool(line, "min_internal_alloc_wifi_rx_matched")
        .ok_or_else(|| {
            anyhow!("PSRAM status missing min_internal_alloc_wifi_rx_matched: {line}")
        })?;
    let minimum_alloc_released = metric_bool(line, "min_internal_alloc_released")
        .ok_or_else(|| anyhow!("PSRAM status missing min_internal_alloc_released: {line}"))?;
    let probe_performed = metric_bool(line, "internal_probe_performed")
        .ok_or_else(|| anyhow!("PSRAM status missing internal_probe_performed: {line}"))?;
    let probe_block = required_u32("internal_probe_block_bytes")?;
    let probe_reserve = required_u32("internal_probe_reserve_bytes")?;
    let probe_before = required_u32("internal_probe_free_before_bytes")?;
    let probe_after = required_u32("internal_probe_free_after_bytes")?;
    let probe_stable = metric_bool(line, "internal_probe_stable")
        .ok_or_else(|| anyhow!("PSRAM status missing internal_probe_stable: {line}"))?;
    let status = ServingAllocatorStatus {
        current_internal,
        minimum_internal,
        minimum_alloc_charge,
        minimum_alloc_internal_required,
        minimum_alloc_charge_overflow,
        minimum_alloc_post_free,
        minimum_alloc_correlation_stable,
        minimum_alloc_wifi_rx_matched,
        minimum_alloc_released,
        probe_performed,
        probe_block,
        probe_reserve,
        probe_before,
        probe_after,
        probe_stable,
    };
    validate_serving_allocator_snapshot(&status)?;
    Ok(status)
}

fn validate_serving_allocator_snapshot(status: &ServingAllocatorStatus) -> Result<()> {
    const INTERNAL_RESERVE_BYTES: u32 = 16 * 1024;
    if !status.minimum_alloc_correlation_stable || !status.minimum_alloc_released {
        return Err(anyhow!(
            "allocator correlation gate failed: current={} minimum={} charge={} internal_required={} charge_overflow={} post_free={} correlation_stable={} wifi_rx_matched={} released={}",
            status.current_internal,
            status.minimum_internal,
            status.minimum_alloc_charge,
            status.minimum_alloc_internal_required,
            status.minimum_alloc_charge_overflow,
            status.minimum_alloc_post_free,
            status.minimum_alloc_correlation_stable,
            status.minimum_alloc_wifi_rx_matched,
            status.minimum_alloc_released
        ));
    }
    if status.minimum_alloc_charge == 0
        || status.minimum_alloc_charge_overflow
        || status.minimum_alloc_post_free != status.minimum_internal
        || status.probe_performed
        || status.probe_block != 0
        || status.probe_reserve != INTERNAL_RESERVE_BYTES
        || status.probe_before != status.probe_after
        || status.probe_before != status.current_internal
        || status.minimum_internal > status.current_internal
        || !status.probe_stable
    {
        return Err(anyhow!(
            "serving allocator status lacked coherent low-water provenance or an allocation-free snapshot: current={} minimum={} charge={} internal_required={} charge_overflow={} post_free={} correlation_stable={} wifi_rx_matched={} released={} performed={} block={} reserve={} before={} after={} stable={}",
            status.current_internal,
            status.minimum_internal,
            status.minimum_alloc_charge,
            status.minimum_alloc_internal_required,
            status.minimum_alloc_charge_overflow,
            status.minimum_alloc_post_free,
            status.minimum_alloc_correlation_stable,
            status.minimum_alloc_wifi_rx_matched,
            status.minimum_alloc_released,
            status.probe_performed,
            status.probe_block,
            status.probe_reserve,
            status.probe_before,
            status.probe_after,
            status.probe_stable
        ));
    }
    Ok(())
}

pub(super) fn metric_u32(line: &str, key: &str) -> Option<u32> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
        .and_then(|value| value.parse::<u32>().ok())
}

pub(super) fn metric_bool(line: &str, key: &str) -> Option<bool> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
        .and_then(|value| match value {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        })
}
