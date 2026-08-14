use super::*;

pub(super) fn stack_status_regex() -> Result<Regex> {
    Ok(Regex::new(
        r"STACK_STATUS cpu0=([0-9]+) touch=([0-9]+) tx_drop=([0-9]+)$",
    )?)
}
pub(super) fn restore_failure_stage(ownership_known: bool) -> &'static str {
    if ownership_known {
        "post_restore_service"
    } else {
        "wifi_restore"
    }
}

pub(super) fn wait_for_post_restore_convergence(
    timeout: Duration,
    poll: Duration,
    mut probe: impl FnMut() -> std::result::Result<(), String>,
) -> Result<u32> {
    let deadline = Instant::now() + timeout;
    let mut attempts = 0u32;
    loop {
        attempts = attempts.saturating_add(1);
        match probe() {
            Ok(()) => return Ok(attempts),
            Err(last_error) => {
                let now = Instant::now();
                if now >= deadline {
                    bail!(
                        "post-restore HTTP did not converge within {} ms after {} attempts: {}",
                        timeout.as_millis(),
                        attempts,
                        last_error
                    );
                }
                thread::sleep(poll.min(deadline.saturating_duration_since(now)));
            }
        }
    }
}

pub(super) fn parse_stack_status(line: &str, regex: &Regex) -> Result<(u32, u32, u32)> {
    let captures = regex
        .captures(line)
        .ok_or_else(|| anyhow!("invalid correlated stack status: {line}"))?;
    let cpu0 = captures
        .get(1)
        .ok_or_else(|| anyhow!("stack status omitted CPU0 minimum"))?
        .as_str()
        .parse::<u32>()?;
    let touch = captures
        .get(2)
        .ok_or_else(|| anyhow!("stack status omitted touch-core minimum"))?
        .as_str()
        .parse::<u32>()?;
    let uart_log_drops = captures
        .get(3)
        .ok_or_else(|| anyhow!("stack status omitted UART diagnostic drop count"))?
        .as_str()
        .parse::<u32>()?;
    Ok((cpu0, touch, uart_log_drops))
}

pub(super) fn validate_stack_floors(cpu0: u32, touch: u32) -> Result<()> {
    if cpu0 < REQUIRED_CPU0_STACK_HEADROOM {
        bail!("CPU0 stack gate failed: headroom={cpu0} floor={REQUIRED_CPU0_STACK_HEADROOM}");
    }
    if touch < REQUIRED_TOUCH_STACK_HEADROOM {
        bail!(
            "touch-core stack gate failed: headroom={touch} floor={REQUIRED_TOUCH_STACK_HEADROOM}"
        );
    }
    Ok(())
}
pub(super) fn validate_uart_drop_counter(baseline: u32, final_count: u32) -> Result<u32> {
    final_count.checked_sub(baseline).ok_or_else(|| {
        anyhow!("UART diagnostic drop counter regressed: baseline={baseline} final={final_count}")
    })
}

pub(super) fn validate_completed_samples(
    samples: &[OffSample],
    requested_cycles: u32,
) -> Result<()> {
    if samples.len() < REQUIRED_GATE_CYCLES as usize || samples.len() != requested_cycles as usize {
        bail!(
            "Phase 1S gate incomplete: requested={requested_cycles} completed={}",
            samples.len()
        );
    }
    for (index, sample) in samples.iter().enumerate() {
        let expected = index as u32 + 1;
        if sample.cycle != expected {
            bail!(
                "Phase 1S cycle sequence is not consecutive: expected={expected} actual={}",
                sample.cycle
            );
        }
    }
    Ok(())
}

pub(super) fn validate_completed_ble_samples(
    samples: &[BleSample],
    requested_cycles: u32,
) -> Result<()> {
    if samples.len() < REQUIRED_GATE_CYCLES as usize || samples.len() != requested_cycles as usize {
        bail!(
            "Phase 1S BLE gate incomplete: requested={requested_cycles} completed={}",
            samples.len()
        );
    }
    for (index, sample) in samples.iter().enumerate() {
        let expected = index as u32 + 1;
        if sample.cycle != expected || sample.epoch != expected {
            bail!(
                "Phase 1S BLE cycle sequence is not consecutive: expected={expected} cycle={} epoch={}",
                sample.cycle,
                sample.epoch
            );
        }
    }
    Ok(())
}

pub(super) fn validate_correlated_samples(off: &[OffSample], ble: &[BleSample]) -> Result<()> {
    if off.len() != ble.len() {
        bail!(
            "Phase 1S off/BLE evidence counts differ: off={} ble={}",
            off.len(),
            ble.len()
        );
    }
    let mut expected_boot = None;
    for (off_sample, ble_sample) in off.iter().zip(ble) {
        if off_sample.cycle != ble_sample.cycle
            || off_sample.epoch != ble_sample.epoch
            || off_sample.boot != ble_sample.boot
        {
            bail!(
                "Phase 1S off/BLE evidence identity mismatch: off={off_sample:?} ble={ble_sample:?}"
            );
        }
        match expected_boot {
            Some(boot) if boot != off_sample.boot => {
                bail!("Phase 1S boot generation changed across completed cycles")
            }
            None => expected_boot = Some(off_sample.boot),
            Some(_) => {}
        }
    }
    Ok(())
}

pub(super) fn validate_off_ack(cycle: u32, boot: u32, ack: &HandoffAck) -> Result<()> {
    if ack.kind != "quiesced"
        || ack.state != "off_confirmed"
        || ack.boot != boot
        || ack.epoch != cycle
    {
        bail!("cycle {cycle}: handoff was not confirmed: {ack:?}");
    }
    validate_off_resources(cycle, ack)
}

pub(super) fn is_known_serving_rejection(cycle: u32, boot: u32, ack: &HandoffAck) -> bool {
    ack.kind == "rejected"
        && ack.state == "serving"
        && matches!(ack.reason.as_str(), "resource_floor" | "quiescence_timeout")
        && ack.boot == boot
        && ack.epoch == cycle
}

pub(super) fn validate_off_status(cycle: u32, boot: u32, ack: &HandoffAck) -> Result<()> {
    if ack.kind != "status"
        || ack.state != "off_confirmed"
        || ack.boot != boot
        || ack.epoch != cycle
    {
        bail!("cycle {cycle}: off state did not remain stable: {ack:?}");
    }
    validate_off_resources(cycle, ack)
}

pub(super) fn validate_off_resources(cycle: u32, ack: &HandoffAck) -> Result<()> {
    if !ack.stable
        || ack.internal_free < REQUIRED_OFF_FREE
        || ack.largest_block < REQUIRED_CONTIGUOUS
        || ack.probe_free_before < REQUIRED_OFF_FREE
        || ack.probe_free_after < REQUIRED_OFF_FREE
        || ack.probe_reserve != 16_384
        || ack.http != 0
        || ack.sd_roundtrip != 0
        || ack.sd_session != 0
        || ack.callbacks != 0
        || ack.queues != 0
        || ack.source_active
        || ack.callback_admission
        || ack.late_callbacks != 0
        || ack.queue_late_use != 0
        || ack.queue_unknown_use != 0
        || ack.queue_reclaim_failures != 0
        || ack.queue_corruption != 0
        || ack.queue_contention != 0
    {
        bail!("cycle {cycle}: Wi-Fi-off resource gate failed: {ack:?}");
    }
    Ok(())
}

pub(super) fn validate_ble_window(
    cycle: u32,
    boot: u32,
    status: &BleWindowStatus,
) -> Result<BleSample> {
    if status.state != "completed"
        || status.failure != "none"
        || status.boot != boot
        || status.epoch != cycle
        || status.before_free < REQUIRED_OFF_FREE
        || status.controller_free < REQUIRED_BLE_ACTIVE_FREE
        || status.active_free < REQUIRED_BLE_ACTIVE_FREE
        || status.after_free < REQUIRED_BLE_ACTIVE_FREE
        || status.callbacks_in_flight != 0
        || status.callback_admission
        || status.callbacks_rejected != 0
        || status.rx_queue_overflow != 0
        || status.rx_oversize != 0
        || status.tx_rejected != 0
        || status.tx_timeout != 0
        || status.queues_active != 0
        || status.queue_late_use != 0
        || status.queue_unknown_use != 0
        || status.queue_reclaim_failures != 0
        || status.queue_corruption != 0
        || status.queue_contention != 0
        || status.queue_task_cancelled > 8
        || status.queue_operation_balance_error != 0
        || status.queue_task_live != 0
        || status.queue_task_faults != 0
        || status.queue_operation_registry_full != 0
        || status.transport_faulted
        || status.packets_free != 4
        || status.pool_exhausted != 0
    {
        bail!("cycle {cycle}: BLE lifecycle/resource gate failed: {status:?}");
    }
    let allocation_delta = i64::from(status.before_free) - i64::from(status.active_free);
    let residual_delta = i64::from(status.before_free) - i64::from(status.after_free);
    if residual_delta.unsigned_abs() > u64::from(MAX_POST_WARMUP_DRIFT) {
        bail!(
            "cycle {cycle}: BLE close did not restore internal memory: residual={residual_delta}"
        );
    }
    Ok(BleSample {
        cycle,
        boot,
        epoch: status.epoch,
        before_free: status.before_free,
        controller_free: status.controller_free,
        active_free: status.active_free,
        after_free: status.after_free,
        allocation_delta,
        residual_delta,
        callbacks_in_flight: status.callbacks_in_flight,
        callback_admission: status.callback_admission,
        callbacks_rejected: status.callbacks_rejected,
        rx_queue_overflow: status.rx_queue_overflow,
        rx_oversize: status.rx_oversize,
        tx_rejected: status.tx_rejected,
        tx_timeout: status.tx_timeout,
        queues_active: status.queues_active,
        queue_late_use: status.queue_late_use,
        queue_unknown_use: status.queue_unknown_use,
        queue_reclaim_failures: status.queue_reclaim_failures,
        queue_corruption: status.queue_corruption,
        queue_contention: status.queue_contention,
        queue_task_cancelled: status.queue_task_cancelled,
        queue_operation_balance_error: status.queue_operation_balance_error,
        queue_task_live: status.queue_task_live,
        queue_task_faults: status.queue_task_faults,
        queue_operation_registry_full: status.queue_operation_registry_full,
        transport_faulted: status.transport_faulted,
        packets_free: status.packets_free,
        pool_exhausted: status.pool_exhausted,
    })
}

pub(super) fn validate_serving_health(cycle: u32, boot: u32, ack: &HandoffAck) -> Result<()> {
    if ack.kind != "status" || ack.state != "serving" || ack.boot != boot || ack.epoch != 0 {
        bail!("cycle {cycle}: owner did not remain healthy after restoration: {ack:?}");
    }
    if ack.late_callbacks != 0
        || ack.queue_late_use != 0
        || ack.queue_unknown_use != 0
        || ack.queue_reclaim_failures != 0
        || ack.queue_corruption != 0
        || ack.queue_contention != 0
    {
        bail!("cycle {cycle}: post-restore lifecycle counters failed: {ack:?}");
    }
    Ok(())
}
