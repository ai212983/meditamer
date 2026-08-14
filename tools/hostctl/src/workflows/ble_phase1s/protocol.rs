use super::*;

#[derive(Clone, Debug, Serialize)]
pub(super) struct OffSample {
    pub(super) cycle: u32,
    pub(super) boot: u32,
    pub(super) epoch: u32,
    pub(super) internal_free: u32,
    pub(super) largest_block: u32,
    pub(super) probe_free_before: u32,
    pub(super) probe_free_after: u32,
    pub(super) probe_reserve: u32,
    pub(super) late_callbacks: u32,
    pub(super) queue_late_use: u32,
    pub(super) queue_unknown_use: u32,
    pub(super) queue_reclaim_failures: u32,
    pub(super) queue_corruption: u32,
    pub(super) queue_contention: u32,
    pub(super) post_restore_late_callbacks: u32,
    pub(super) post_restore_queue_late_use: u32,
    pub(super) post_restore_queue_unknown_use: u32,
    pub(super) post_restore_queue_reclaim_failures: u32,
    pub(super) post_restore_queue_corruption: u32,
    pub(super) post_restore_queue_contention: u32,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct BleSample {
    pub(super) cycle: u32,
    pub(super) boot: u32,
    pub(super) epoch: u32,
    pub(super) before_free: u32,
    pub(super) controller_free: u32,
    pub(super) active_free: u32,
    pub(super) after_free: u32,
    pub(super) allocation_delta: i64,
    pub(super) residual_delta: i64,
    pub(super) callbacks_in_flight: u32,
    pub(super) callback_admission: bool,
    pub(super) callbacks_rejected: u32,
    pub(super) rx_queue_overflow: u32,
    pub(super) rx_oversize: u32,
    pub(super) tx_rejected: u32,
    pub(super) tx_timeout: u32,
    pub(super) queues_active: u32,
    pub(super) queue_late_use: u32,
    pub(super) queue_unknown_use: u32,
    pub(super) queue_reclaim_failures: u32,
    pub(super) queue_corruption: u32,
    pub(super) queue_contention: u32,
    pub(super) queue_task_cancelled: u32,
    pub(super) queue_operation_balance_error: u32,
    pub(super) queue_task_live: u32,
    pub(super) queue_task_faults: u32,
    pub(super) queue_operation_registry_full: u32,
    pub(super) transport_faulted: bool,
    pub(super) packets_free: u8,
    pub(super) pool_exhausted: u32,
}

#[derive(Debug, Serialize)]
pub(super) struct Phase1sReport {
    pub(super) schema_version: u8,
    pub(super) gate_kind: &'static str,
    pub(super) board_id: String,
    pub(super) build_id: String,
    pub(super) source_git_head: String,
    pub(super) artifact_elf_sha256: String,
    pub(super) artifact_app_sha256: String,
    pub(super) serial_port: String,
    pub(super) network_ssid: String,
    pub(super) completed_cycles: u32,
    pub(super) completed_ble_cycles: u32,
    pub(super) first_ble_allocation_delta_bytes: Option<i64>,
    pub(super) minimum_ble_active_internal_free_bytes: Option<u32>,
    pub(super) minimum_serving_internal_free_bytes: Option<u32>,
    pub(super) minimum_serving_internal_alloc_charge_bytes: Option<u32>,
    pub(super) minimum_serving_internal_alloc_internal_required: Option<bool>,
    pub(super) minimum_serving_internal_alloc_wifi_rx_matched: Option<bool>,
    pub(super) minimum_serving_internal_alloc_correlation_stable: Option<bool>,
    pub(super) minimum_serving_internal_alloc_released: Option<bool>,
    pub(super) post_warmup_free_drift: u32,
    pub(super) post_warmup_largest_block_drift: u32,
    pub(super) cpu0_stack_headroom_min: Option<u32>,
    pub(super) touch_stack_headroom_min: Option<u32>,
    pub(super) uart_log_drops_baseline: Option<u32>,
    pub(super) uart_log_drops_final: Option<u32>,
    pub(super) uart_log_drops_during_gate: Option<u32>,
    pub(super) failure_stage: Option<String>,
    pub(super) failure_reason: Option<String>,
    pub(super) ownership_known: Option<bool>,
    pub(super) pending_off: Option<HandoffAck>,
    pub(super) pending_ble_status: Option<BleWindowStatus>,
    pub(super) gate_passed: bool,
    pub(super) violations: Vec<String>,
    pub(super) off_samples: Vec<OffSample>,
    pub(super) ble_samples: Vec<BleSample>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct BleWindowStatus {
    pub(super) state: String,
    pub(super) failure: String,
    pub(super) build_id: String,
    pub(super) boot: u32,
    pub(super) epoch: u32,
    pub(super) before_free: u32,
    pub(super) controller_free: u32,
    pub(super) active_free: u32,
    pub(super) after_free: u32,
    pub(super) callbacks_in_flight: u32,
    pub(super) callback_admission: bool,
    pub(super) callbacks_rejected: u32,
    pub(super) rx_queue_overflow: u32,
    pub(super) rx_oversize: u32,
    pub(super) tx_rejected: u32,
    pub(super) tx_timeout: u32,
    pub(super) queues_active: u32,
    pub(super) queue_late_use: u32,
    pub(super) queue_unknown_use: u32,
    pub(super) queue_reclaim_failures: u32,
    pub(super) queue_corruption: u32,
    pub(super) queue_contention: u32,
    pub(super) queue_task_cancelled: u32,
    pub(super) queue_operation_balance_error: u32,
    pub(super) queue_task_live: u32,
    pub(super) queue_task_faults: u32,
    pub(super) queue_operation_registry_full: u32,
    pub(super) transport_faulted: bool,
    pub(super) packets_free: u8,
    pub(super) pool_exhausted: u32,
}

pub(super) struct PendingCycle {
    pub(super) cycle: u32,
    pub(super) boot: u32,
    pub(super) off: HandoffAck,
    pub(super) ble: Option<BleSample>,
    pub(super) ble_status: Option<BleWindowStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct HandoffAck {
    pub(super) kind: String,
    pub(super) state: String,
    pub(super) reason: String,
    pub(super) boot: u32,
    pub(super) epoch: u32,
    pub(super) internal_free: u32,
    pub(super) largest_block: u32,
    pub(super) probe_free_before: u32,
    pub(super) probe_free_after: u32,
    pub(super) probe_reserve: u32,
    pub(super) http: u16,
    pub(super) sd_roundtrip: u16,
    pub(super) sd_session: u16,
    pub(super) callbacks: u16,
    pub(super) queues: u16,
    pub(super) source_active: bool,
    pub(super) callback_admission: bool,
    pub(super) late_callbacks: u32,
    pub(super) queue_late_use: u32,
    pub(super) queue_unknown_use: u32,
    pub(super) queue_reclaim_failures: u32,
    pub(super) queue_corruption: u32,
    pub(super) queue_contention: u32,
    pub(super) stable: bool,
}

pub(super) fn ack_regex() -> Result<Regex> {
    Regex::new(concat!(
        // The firmware emits the entire acknowledgement with one locked
        // Printer::write_bytes call, so the marker and payload stay
        // contiguous. A formatter on the other core can still leave a
        // diagnostic fragment before that atomic write. Accept that prefix,
        // while keeping the complete acknowledgement anchored at line end.
        r"RADIO_HANDOFF_ACK kind=([a-z_]+) state=([a-z_]+) reason=([a-z_]+) ",
        r"boot=([0-9]+) epoch=([0-9]+) internal_free=([0-9]+) ",
        r"block_above_reserve=([0-9]+) probe_before=([0-9]+) probe_after=([0-9]+) ",
        r"probe_reserve=([0-9]+) http=([0-9]+) sd_roundtrip=([0-9]+) ",
        r"sd_session=([0-9]+) callbacks=([0-9]+) queues=([0-9]+) ",
        r"source_active=(true|false) callback_admission=(true|false) late_callbacks=([0-9]+) ",
        r"queue_late=([0-9]+) queue_unknown=([0-9]+) queue_reclaim_fail=([0-9]+) ",
        r"queue_corruption=([0-9]+) queue_contention=([0-9]+) stable=(true|false)$"
    ))
    .map_err(Into::into)
}

pub(super) fn parse_ack(line: &str, regex: &Regex) -> Result<HandoffAck> {
    let captures = regex
        .captures(line)
        .ok_or_else(|| anyhow!("invalid RADIO_HANDOFF_ACK: {line}"))?;
    let number = |index: usize| -> Result<u32> {
        captures[index]
            .parse::<u32>()
            .with_context(|| format!("invalid acknowledgement number in {line}"))
    };
    Ok(HandoffAck {
        kind: captures[1].to_owned(),
        state: captures[2].to_owned(),
        reason: captures[3].to_owned(),
        boot: number(4)?,
        epoch: number(5)?,
        internal_free: number(6)?,
        largest_block: number(7)?,
        probe_free_before: number(8)?,
        probe_free_after: number(9)?,
        probe_reserve: number(10)?,
        http: number(11)?.try_into()?,
        sd_roundtrip: number(12)?.try_into()?,
        sd_session: number(13)?.try_into()?,
        callbacks: number(14)?.try_into()?,
        queues: number(15)?.try_into()?,
        source_active: &captures[16] == "true",
        callback_admission: &captures[17] == "true",
        late_callbacks: number(18)?,
        queue_late_use: number(19)?,
        queue_unknown_use: number(20)?,
        queue_reclaim_failures: number(21)?,
        queue_corruption: number(22)?,
        queue_contention: number(23)?,
        stable: &captures[24] == "true",
    })
}

pub(super) fn ble_window_ack_regex() -> Result<Regex> {
    Ok(Regex::new(
        r"BLE_P1S_ACK kind=(queued|rejected) reason=([a-z_]+) boot=([0-9]+) epoch=([0-9]+)$",
    )?)
}

pub(super) fn ble_window_status_regex() -> Result<Regex> {
    Ok(Regex::new(concat!(
        r"BLE_P1S_STATUS state=([a-z_]+) failure=([a-z_]+) build_id=([A-Za-z0-9._-]+) ",
        r"boot=([0-9]+) epoch=([0-9]+) before=([0-9]+) controller=([0-9]+) ",
        r"active=([0-9]+) after=([0-9]+) callbacks=([0-9]+) admission=(true|false) ",
        r"rejected=([0-9]+) rx_overflow=([0-9]+) rx_oversize=([0-9]+) ",
        r"tx_rejected=([0-9]+) tx_timeout=([0-9]+) queues=([0-9]+) queue_late=([0-9]+) ",
        r"queue_unknown=([0-9]+) queue_reclaim=([0-9]+) queue_corruption=([0-9]+) ",
        r"queue_contention=([0-9]+) queue_task_cancelled=([0-9]+) queue_balance=([0-9]+) ",
        r"queue_task_live=([0-9]+) queue_task_faults=([0-9]+) queue_op_full=([0-9]+) ",
        r"transport_faulted=(true|false) packets_free=([0-9]+) ",
        r"pool_exhausted=([0-9]+) coex=false$"
    ))?)
}

pub(super) fn parse_ble_window_status(line: &str, regex: &Regex) -> Result<BleWindowStatus> {
    let captures = regex
        .captures(line)
        .ok_or_else(|| anyhow!("invalid BLE_P1S_STATUS: {line}"))?;
    let number = |index: usize| -> Result<u32> {
        captures[index]
            .parse::<u32>()
            .with_context(|| format!("invalid BLE window number in {line}"))
    };
    Ok(BleWindowStatus {
        state: captures[1].to_owned(),
        failure: captures[2].to_owned(),
        build_id: captures[3].to_owned(),
        boot: number(4)?,
        epoch: number(5)?,
        before_free: number(6)?,
        controller_free: number(7)?,
        active_free: number(8)?,
        after_free: number(9)?,
        callbacks_in_flight: number(10)?,
        callback_admission: &captures[11] == "true",
        callbacks_rejected: number(12)?,
        rx_queue_overflow: number(13)?,
        rx_oversize: number(14)?,
        tx_rejected: number(15)?,
        tx_timeout: number(16)?,
        queues_active: number(17)?,
        queue_late_use: number(18)?,
        queue_unknown_use: number(19)?,
        queue_reclaim_failures: number(20)?,
        queue_corruption: number(21)?,
        queue_contention: number(22)?,
        queue_task_cancelled: number(23)?,
        queue_operation_balance_error: number(24)?,
        queue_task_live: number(25)?,
        queue_task_faults: number(26)?,
        queue_operation_registry_full: number(27)?,
        transport_faulted: &captures[28] == "true",
        packets_free: number(29)?.try_into()?,
        pool_exhausted: number(30)?,
    })
}

pub(super) fn drift(values: impl Iterator<Item = u32>) -> u32 {
    let mut minimum = u32::MAX;
    let mut maximum = 0;
    let mut count = 0;
    for value in values {
        minimum = minimum.min(value);
        maximum = maximum.max(value);
        count += 1;
    }
    if count == 0 {
        0
    } else {
        maximum.saturating_sub(minimum)
    }
}
pub(super) fn rejected_ble_outcome(reason: &str) -> (&'static str, bool) {
    match reason {
        // Busy means the device observed Queued or Running. Restoring Wi-Fi
        // would race the active BLE owner, so only reboot/status recovery is safe.
        "busy" | "ownership_unknown" => ("ownership_unknown", false),
        "consumed" | "exclusive_lease" | "update_reserved" => ("known_failed", true),
        _ => ("ownership_unknown", false),
    }
}
