use super::{
    ack_regex, allocator_status_regex, ble_window_status_regex, build_phase1s_netcfg_command,
    collect_stack_metrics, drift, is_known_serving_rejection, parse_ack, parse_allocator_minimum,
    parse_ble_window_status, parse_stack_status, rejected_ble_outcome, restore_failure_stage,
    stack_status_regex, validate_allocator_minimum, validate_ble_window,
    validate_completed_ble_samples, validate_completed_samples, validate_correlated_samples,
    validate_gate_options, validate_off_resources, validate_stack_floors,
    validate_uart_drop_counter, wait_for_post_restore_convergence, ArtifactIdentity,
    BlePhase1sRuntime, BleSample, OffSample, PendingCycle, NETCFG_TX_CHUNK_BYTES,
    NETCFG_TX_SETTLE_MS,
};
use crate::{
    logging::Logger,
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::SerialConsole,
    workflows::wifi::common::{acquire_port_lock, NetPolicy},
};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use serialport::{SerialPort, TTYPort};
use std::{
    io::{Read, Write},
    path::PathBuf,
    thread,
    time::Duration,
};

#[derive(Default)]
struct RecordingRuntime {
    calls: Vec<String>,
    fail_action: Option<String>,
    handoff_outcome: Option<String>,
    ble_outcome: Option<String>,
    restore_outcome: Option<String>,
    metrics_outcome: Option<String>,
}

impl WorkflowRuntime for RecordingRuntime {
    fn invoke(&mut self, action: &str, _args: &Value, _context: &mut Value) -> Result<()> {
        self.calls.push(action.to_owned());
        if self.fail_action.as_deref() == Some(action) {
            return Err(anyhow!("injected action failure: {action}"));
        }
        Ok(())
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        self.invoke(action, args, context)?;
        Ok(match action {
            "init_run" => Some(json!({ "cycle_count": 2 })),
            "prepare_off_window" => Some(json!({
                "handoff_outcome": self.handoff_outcome.as_deref().unwrap_or("pass"),
                "handoff_reason": if self.handoff_outcome.as_deref().unwrap_or("pass") == "pass" { "none" } else { "injected" },
            })),
            "run_ble_window" => Some(json!({
                "ble_outcome": self.ble_outcome.as_deref().unwrap_or("pass"),
                "ble_reason": if self.ble_outcome.as_deref() == Some("pass") { "none" } else { "injected" },
                "ble_ownership_known": self.ble_outcome.as_deref() != Some("ownership_unknown"),
            })),
            "restore_and_complete_cycle" => Some(json!({
                "restore_outcome": self.restore_outcome.as_deref().unwrap_or("pass"),
                "restore_reason": if self.restore_outcome.as_deref().unwrap_or("pass") == "pass" { "none" } else { "injected" },
            })),
            "collect_stack_metrics" => Some(json!({
                "metrics_outcome": self.metrics_outcome.as_deref().unwrap_or("pass"),
                "metrics_reason": if self.metrics_outcome.as_deref().unwrap_or("pass") == "pass" { "none" } else { "injected" },
            })),
            _ => None,
        })
    }
}

fn spawn_netcfg_responder(
    mut master: TTYPort,
    response: &'static [u8],
) -> thread::JoinHandle<Vec<String>> {
    let _ = master.set_timeout(Duration::from_millis(80));
    thread::spawn(move || {
        let mut commands = Vec::new();
        let mut pending = Vec::new();
        let mut chunk = [0u8; 512];
        let mut responded = false;
        loop {
            match master.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => pending.extend_from_slice(&chunk[..n]),
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::Interrupted
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                Err(_) => break,
            }
            while let Some(pos) = pending.iter().position(|byte| *byte == b'\n') {
                let mut line = pending.drain(..=pos).collect::<Vec<_>>();
                while matches!(line.last(), Some(b'\r' | b'\n')) {
                    line.pop();
                }
                if line.is_empty() {
                    continue;
                }
                commands.push(String::from_utf8_lossy(&line).to_string());
                if !responded {
                    master
                        .write_all(response)
                        .expect("write NET acknowledgement");
                    master.flush().expect("flush NET acknowledgement");
                    responded = true;
                }
            }
        }
        commands
    })
}

fn spawn_scripted_responder(
    mut master: TTYPort,
    responses: Vec<Vec<u8>>,
) -> thread::JoinHandle<Vec<String>> {
    let _ = master.set_timeout(Duration::from_millis(80));
    thread::spawn(move || {
        let mut commands = Vec::new();
        let mut pending = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            match master.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => pending.extend_from_slice(&chunk[..n]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::Interrupted
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                Err(_) => break,
            }
            while let Some(pos) = pending.iter().position(|byte| *byte == b'\n') {
                let mut line = pending.drain(..=pos).collect::<Vec<_>>();
                while matches!(line.last(), Some(b'\r' | b'\n')) {
                    line.pop();
                }
                if line.is_empty() {
                    continue;
                }
                commands.push(String::from_utf8_lossy(&line).to_string());
                if let Some(response) = responses.get(commands.len() - 1) {
                    master.write_all(response).expect("write scripted response");
                    master.flush().expect("flush scripted response");
                }
            }
        }
        commands
    })
}

fn netcfg_test_runtime<'a>(
    logger: &'a mut Logger,
    slave: TTYPort,
    case: &str,
) -> Result<BlePhase1sRuntime<'a>> {
    let port = format!("phase1s-netcfg-{case}-{}", std::process::id());
    Ok(BlePhase1sRuntime {
        logger,
        console: SerialConsole::from_port_for_tests(Box::new(slave), None)?,
        _port_lock: acquire_port_lock(&port)?,
        identity: ArtifactIdentity {
            build_id: "test-build".to_owned(),
            elf_sha256: "test-elf".to_owned(),
            app_sha256: "test-app".to_owned(),
            git_head: "test-head".to_owned(),
        },
        board_id: "test-board".to_owned(),
        port,
        cycles: 20,
        ssid: "test-ssid".to_owned(),
        policy: NetPolicy::default(),
        netcfg_command: build_phase1s_netcfg_command(
            "test-ssid",
            "test-password",
            NetPolicy::default(),
        )?,
        payload_path: PathBuf::from("unused-payload.bin"),
        report_path: PathBuf::from("unused-report.json"),
        evidence_mark: 0,
        boot_generation: None,
        cpu0_stack_headroom_min: None,
        touch_stack_headroom_min: None,
        serving_internal_free_min: Some(16_384),
        serving_internal_min_alloc_charge: Some(1_700),
        serving_internal_min_alloc_internal_required: Some(true),
        serving_internal_min_alloc_wifi_rx_matched: Some(true),
        serving_internal_min_alloc_correlation_stable: Some(true),
        serving_internal_min_alloc_released: Some(true),
        uart_log_drops_baseline: None,
        uart_log_drops_final: None,
        samples: Vec::new(),
        ble_samples: Vec::new(),
        pending_cycle: None,
        known_serving_rejection: false,
        failure_stage: None,
        failure_reason: None,
        ownership_known: None,
    })
}

fn passing_ack() -> String {
    "RADIO_HANDOFF_ACK kind=quiesced state=off_confirmed reason=none boot=17 epoch=4 internal_free=22000 block_above_reserve=5000 probe_before=22000 probe_after=22000 probe_reserve=16384 http=0 sd_roundtrip=0 sd_session=0 callbacks=0 queues=0 source_active=false callback_admission=false late_callbacks=0 queue_late=0 queue_unknown=0 queue_reclaim_fail=0 queue_corruption=0 queue_contention=0 stable=true".to_owned()
}

fn sample(cycle: u32) -> OffSample {
    OffSample {
        cycle,
        boot: 17,
        epoch: cycle,
        internal_free: 22_000,
        largest_block: 5_000,
        probe_free_before: 22_000,
        probe_free_after: 22_000,
        probe_reserve: 16_384,
        late_callbacks: 0,
        queue_late_use: 0,
        queue_unknown_use: 0,
        queue_reclaim_failures: 0,
        queue_corruption: 0,
        queue_contention: 0,
        post_restore_late_callbacks: 0,
        post_restore_queue_late_use: 0,
        post_restore_queue_unknown_use: 0,
        post_restore_queue_reclaim_failures: 0,
        post_restore_queue_corruption: 0,
        post_restore_queue_contention: 0,
    }
}

fn ble_sample(cycle: u32) -> BleSample {
    BleSample {
        cycle,
        boot: 17,
        epoch: cycle,
        before_free: 30_000,
        controller_free: 24_000,
        active_free: 20_000,
        after_free: 30_000,
        allocation_delta: 10_000,
        residual_delta: 0,
        callbacks_in_flight: 0,
        callback_admission: false,
        callbacks_rejected: 0,
        rx_queue_overflow: 0,
        rx_oversize: 0,
        tx_rejected: 0,
        tx_timeout: 0,
        queues_active: 0,
        queue_late_use: 0,
        queue_unknown_use: 0,
        queue_reclaim_failures: 0,
        queue_corruption: 0,
        queue_contention: 0,
        queue_task_cancelled: 1,
        queue_operation_balance_error: 0,
        queue_task_live: 0,
        queue_task_faults: 0,
        queue_operation_registry_full: 0,
        transport_faulted: false,
        packets_free: 4,
        pool_exhausted: 0,
    }
}

fn passing_ble_status() -> String {
    "BLE_P1S_STATUS state=completed failure=none build_id=test-build boot=17 epoch=4 before=30000 controller=24000 active=20000 after=30000 callbacks=0 admission=false rejected=0 rx_overflow=0 rx_oversize=0 tx_rejected=0 tx_timeout=0 queues=0 queue_late=0 queue_unknown=0 queue_reclaim=0 queue_corruption=0 queue_contention=0 queue_task_cancelled=1 queue_balance=0 queue_task_live=0 queue_task_faults=0 queue_op_full=0 transport_faulted=false packets_free=4 pool_exhausted=0 coex=false".to_owned()
}

#[test]
fn ble_window_status_is_correlated_and_enforces_the_active_floor() {
    let regex = ble_window_status_regex().expect("regex");
    let status = parse_ble_window_status(&passing_ble_status(), &regex).expect("status");
    let sample = validate_ble_window(4, 17, &status).expect("BLE window");
    assert_eq!(sample.allocation_delta, 10_000);
    assert_eq!(sample.residual_delta, 0);

    let below = passing_ble_status().replace("active=20000", "active=16383");
    let status = parse_ble_window_status(&below, &regex).expect("below-floor status");
    assert!(validate_ble_window(4, 17, &status).is_err());

    let stale = passing_ble_status().replace("epoch=4", "epoch=3");
    let status = parse_ble_window_status(&stale, &regex).expect("stale status");
    assert!(validate_ble_window(4, 17, &status).is_err());
}

#[test]
fn ble_window_rejects_unbalanced_or_unattributed_task_shutdown() {
    let regex = ble_window_status_regex().expect("regex");
    let passing = parse_ble_window_status(&passing_ble_status(), &regex).expect("status");

    let mut zero_cancel = passing.clone();
    zero_cancel.queue_task_cancelled = 0;
    assert!(validate_ble_window(4, 17, &zero_cancel).is_ok());
    let mut maximum_cancel = passing.clone();
    maximum_cancel.queue_task_cancelled = 8;
    assert!(validate_ble_window(4, 17, &maximum_cancel).is_ok());

    let mut cases = Vec::new();
    let mut status = passing.clone();
    status.queue_task_cancelled = 9;
    cases.push(status);
    let mut status = passing.clone();
    status.queue_operation_balance_error = 1;
    cases.push(status);
    let mut status = passing.clone();
    status.queue_task_live = 1;
    cases.push(status);
    let mut status = passing.clone();
    status.queue_task_faults = 1;
    cases.push(status);
    let mut status = passing;
    status.queue_operation_registry_full = 1;
    cases.push(status);

    for status in cases {
        assert!(validate_ble_window(4, 17, &status).is_err(), "{status:?}");
    }
}

#[test]
fn max_width_queue_lifecycle_terminal_fits_the_guarded_status_envelope() {
    let maximum = u32::MAX;
    let line = format!(
        "BLE_P1S_STATUS state=ownership_unknown failure=queue_lifecycle build_id={} boot={maximum} epoch={maximum} before={maximum} controller={maximum} active={maximum} after={maximum} callbacks={maximum} admission=false rejected={maximum} rx_overflow={maximum} rx_oversize={maximum} tx_rejected={maximum} tx_timeout={maximum} queues={maximum} queue_late={maximum} queue_unknown={maximum} queue_reclaim={maximum} queue_corruption={maximum} queue_contention={maximum} queue_task_cancelled={maximum} queue_balance={maximum} queue_task_live={maximum} queue_task_faults={maximum} queue_op_full={maximum} transport_faulted=true packets_free=255 pool_exhausted={maximum} coex=false",
        "b".repeat(31)
    );
    assert!(line.len() > 512, "fixture must cover the former overflow");
    assert!(
        line.len() < 768,
        "guarded firmware capacity must cover fixture"
    );
    let parsed = parse_ble_window_status(&line, &ble_window_status_regex().unwrap()).unwrap();
    assert_eq!(parsed.state, "ownership_unknown");
    assert_eq!(parsed.failure, "queue_lifecycle");
    assert_eq!(parsed.queue_task_faults, maximum);
}

#[test]
fn report_requires_twenty_matching_ble_windows() {
    let mut samples = (1..=20).map(ble_sample).collect::<Vec<_>>();
    validate_completed_ble_samples(&samples, 20).expect("twenty BLE samples");
    samples.pop();
    assert!(validate_completed_ble_samples(&samples, 20).is_err());
    let mut samples = (1..=20).map(ble_sample).collect::<Vec<_>>();
    samples[8].epoch = 8;
    assert!(validate_completed_ble_samples(&samples, 20).is_err());
}

#[test]
fn report_requires_exact_off_ble_identity_correlation() {
    let off: Vec<_> = (1..=20).map(sample).collect();
    let mut ble: Vec<_> = (1..=20).map(ble_sample).collect();
    validate_correlated_samples(&off, &ble).expect("matching evidence");
    ble[7].boot += 1;
    assert!(validate_correlated_samples(&off, &ble).is_err());
}

#[test]
fn acknowledgement_parser_and_resource_gate_accept_complete_off_state() {
    let regex = ack_regex().expect("regex");
    let ack = parse_ack(&passing_ack(), &regex).expect("parse");
    validate_off_resources(4, &ack).expect("resources");
    assert_eq!(ack.boot, 17);
    assert_eq!(ack.epoch, 4);
}

#[test]
fn acknowledgement_parser_accepts_atomic_ack_after_diagnostic_fragment() {
    let regex = ack_regex().expect("regex");
    let line = format!("stack_diag: tag=sd_fat_metadata_io{}", passing_ack());
    let ack = parse_ack(&line, &regex).expect("parse atomic acknowledgement suffix");

    validate_off_resources(4, &ack).expect("resources");
    assert_eq!(ack.boot, 17);
    assert_eq!(ack.epoch, 4);
}

#[test]
fn acknowledgement_parser_rejects_text_after_structured_ack() {
    let regex = ack_regex().expect("regex");
    let line = format!("{}trailing diagnostic", passing_ack());

    assert!(parse_ack(&line, &regex).is_err());
}

#[test]
fn resource_gate_rejects_each_nonzero_owner_counter() {
    let regex = ack_regex().expect("regex");
    for field in [
        "http",
        "sd_roundtrip",
        "sd_session",
        "callbacks",
        "queues",
        "late_callbacks",
        "queue_late",
        "queue_unknown",
        "queue_reclaim_fail",
        "queue_corruption",
        "queue_contention",
    ] {
        let line = passing_ack().replace(&format!("{field}=0"), &format!("{field}=1"));
        let ack = parse_ack(&line, &regex).expect("parse");
        assert!(validate_off_resources(4, &ack).is_err(), "{field}");
    }
}

#[test]
fn resource_gate_rejects_live_source_or_callback_admission() {
    let regex = ack_regex().expect("regex");
    for field in ["source_active", "callback_admission"] {
        let line = passing_ack().replace(&format!("{field}=false"), &format!("{field}=true"));
        let ack = parse_ack(&line, &regex).expect("parse");
        assert!(validate_off_resources(4, &ack).is_err(), "{field}");
    }
}

#[test]
fn resource_gate_rejects_unrestored_or_under_reserved_probe() {
    let regex = ack_regex().expect("regex");
    for line in [
        passing_ack().replace("probe_after=22000", "probe_after=20495"),
        passing_ack().replace("probe_reserve=16384", "probe_reserve=16383"),
        passing_ack().replace("stable=true", "stable=false"),
    ] {
        let ack = parse_ack(&line, &regex).expect("parse");
        assert!(validate_off_resources(4, &ack).is_err(), "{line}");
    }
}

#[test]
fn drift_is_maximum_minus_minimum() {
    assert_eq!(drift([22_000, 21_500, 22_100].into_iter()), 600);
    assert_eq!(drift([].into_iter()), 0);
}

#[test]
fn runtime_stack_gate_enforces_both_plan_floors() {
    validate_stack_floors(8_192, 1_024).expect("exact floors");
    assert!(validate_stack_floors(8_191, 1_024).is_err());
    assert!(validate_stack_floors(8_192, 1_023).is_err());
}

#[test]
fn uart_drop_counter_requires_a_monotonic_zero_delta() {
    assert_eq!(validate_uart_drop_counter(5, 5).expect("zero delta"), 0);
    assert_eq!(validate_uart_drop_counter(5, 6).expect("one drop"), 1);
    assert!(validate_uart_drop_counter(6, 5).is_err());
}

#[test]
fn stack_status_parser_uses_the_matched_suffix_captures() {
    let regex = stack_status_regex().expect("regex");
    assert_eq!(
        parse_stack_status("STACK_STATUS cpu0=8192 touch=1024 tx_drop=3", &regex).expect("status"),
        (8_192, 1_024, 3)
    );
    assert_eq!(
        parse_stack_status(
            "stack_diag: headroom=12000 touch=9000STACK_STATUS cpu0=7000 touch=800 tx_drop=4",
            &regex,
        )
        .expect("authoritative suffix"),
        (7_000, 800, 4)
    );
    assert!(parse_stack_status("stack_diag: headroom=12000", &regex).is_err());
    assert!(parse_stack_status(
        "STACK_STATUS cpu0=8192 touch=1024 tx_drop=0 trailing",
        &regex
    )
    .is_err());
}

#[test]
fn allocator_status_parser_binds_the_run_wide_minimum_and_allocation_free_snapshot() {
    let regex = allocator_status_regex().expect("regex");
    let line = "PSRAM feature_enabled=true state=Initialized total_bytes=1 used_bytes=2 free_bytes=3 peak_used_bytes=4 internal_free_bytes=17000 external_free_bytes=5 min_free_bytes=6 min_internal_free_bytes=16384 min_internal_alloc_charge_bytes=1700 min_internal_alloc_internal_required=true min_internal_alloc_charge_overflow=false min_internal_alloc_post_free_bytes=16384 min_internal_alloc_correlation_stable=true min_internal_alloc_wifi_rx_matched=true min_internal_alloc_released=true min_external_free_bytes=7 large_alloc_external_ok=0 large_alloc_internal_ok=0 large_alloc_fail=0 internal_probe_performed=false internal_probe_block_bytes=0 internal_probe_reserve_bytes=16384 internal_probe_free_before_bytes=17000 internal_probe_free_after_bytes=17000 internal_probe_stable=true";
    assert_eq!(
        parse_allocator_minimum(line, &regex).unwrap().free_bytes,
        16_384
    );
    for invalid in [
        line.replace(
            "min_internal_alloc_charge_bytes=1700",
            "min_internal_alloc_charge_bytes=0",
        ),
        line.replace(
            "min_internal_alloc_internal_required=true",
            "min_internal_alloc_internal_required=invalid",
        ),
        line.replace(
            "min_internal_alloc_charge_overflow=false",
            "min_internal_alloc_charge_overflow=true",
        ),
        line.replace(
            "min_internal_alloc_correlation_stable=true",
            "min_internal_alloc_correlation_stable=false",
        ),
        line.replace(
            "min_internal_alloc_released=true",
            "min_internal_alloc_released=false",
        ),
        line.replace(
            "min_internal_alloc_post_free_bytes=16384",
            "min_internal_alloc_post_free_bytes=16392",
        ),
        line.replace(
            "internal_probe_performed=false",
            "internal_probe_performed=true",
        ),
        line.replace(
            "internal_probe_block_bytes=0",
            "internal_probe_block_bytes=4112",
        ),
        line.replace(
            "internal_probe_reserve_bytes=16384",
            "internal_probe_reserve_bytes=8192",
        ),
        line.replace(
            "internal_probe_free_after_bytes=17000",
            "internal_probe_free_after_bytes=16992",
        ),
        line.replace(
            "internal_probe_free_before_bytes=17000",
            "internal_probe_free_before_bytes=16992",
        ),
        line.replace(
            "min_internal_free_bytes=16384",
            "min_internal_free_bytes=18000",
        ),
        line.replace("internal_probe_stable=true", "internal_probe_stable=false"),
    ] {
        if let Ok(parsed) = parse_allocator_minimum(&invalid, &regex) {
            assert!(validate_allocator_minimum(&parsed).is_err());
        }
    }
    assert!(parse_allocator_minimum(&format!("{line} trailing"), &regex).is_err());
}

#[test]
fn allocator_correlation_failure_fields_survive_into_failure_report() -> Result<()> {
    let regex = allocator_status_regex()?;
    let line = "PSRAM feature_enabled=true state=Initialized total_bytes=1 used_bytes=2 free_bytes=3 peak_used_bytes=4 internal_free_bytes=17000 external_free_bytes=5 min_free_bytes=6 min_internal_free_bytes=16384 min_internal_alloc_charge_bytes=1700 min_internal_alloc_internal_required=true min_internal_alloc_charge_overflow=false min_internal_alloc_post_free_bytes=16384 min_internal_alloc_correlation_stable=false min_internal_alloc_wifi_rx_matched=false min_internal_alloc_released=false min_external_free_bytes=7 large_alloc_external_ok=0 large_alloc_internal_ok=0 large_alloc_fail=0 internal_probe_performed=false internal_probe_block_bytes=0 internal_probe_reserve_bytes=16384 internal_probe_free_before_bytes=17000 internal_probe_free_after_bytes=17000 internal_probe_stable=true";
    let parsed = parse_allocator_minimum(line, &regex)?;
    assert!(!parsed.correlation_stable);
    assert!(!parsed.wifi_rx_matched);
    assert!(!parsed.released);
    assert!(validate_allocator_minimum(&parsed).is_err());

    let (_master, slave) =
        TTYPort::pair().map_err(|error| anyhow!("TTYPort::pair failed: {error}"))?;
    let mut logger = Logger::new(None)?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "correlation-failure")?;
    runtime.serving_internal_free_min = Some(parsed.free_bytes);
    runtime.serving_internal_min_alloc_charge = Some(parsed.charge_bytes);
    runtime.serving_internal_min_alloc_internal_required = Some(parsed.internal_required);
    runtime.serving_internal_min_alloc_correlation_stable = Some(parsed.correlation_stable);
    runtime.serving_internal_min_alloc_wifi_rx_matched = Some(parsed.wifi_rx_matched);
    runtime.serving_internal_min_alloc_released = Some(parsed.released);
    runtime.failure_reason = Some("allocator correlation gate failed".to_owned());
    let report = serde_json::to_value(runtime.finish_failure_report())?;
    assert_eq!(
        report["minimum_serving_internal_alloc_correlation_stable"],
        false
    );
    assert_eq!(
        report["minimum_serving_internal_alloc_wifi_rx_matched"],
        false
    );
    assert_eq!(report["minimum_serving_internal_alloc_released"], false);
    Ok(())
}

#[test]
fn post_restore_convergence_retries_without_releasing_again() {
    let mut attempts = 0;
    let completed = wait_for_post_restore_convergence(
        Duration::from_millis(50),
        Duration::from_millis(1),
        || {
            attempts += 1;
            (attempts >= 3)
                .then_some(())
                .ok_or_else(|| "not reachable yet".to_owned())
        },
    )
    .expect("third health probe converges");
    assert_eq!(completed, 3);
    assert_eq!(attempts, 3);
}

#[test]
fn post_restore_timeout_preserves_known_ownership_classification() {
    let error = wait_for_post_restore_convergence(
        Duration::from_millis(3),
        Duration::from_millis(1),
        || Err("still unreachable".to_owned()),
    )
    .expect_err("bounded convergence must fail");
    assert!(error.to_string().contains("still unreachable"));
    assert_eq!(restore_failure_stage(true), "post_restore_service");
    assert_eq!(restore_failure_stage(false), "wifi_restore");
}

#[test]
fn restored_ownership_revalidation_requires_same_boot_without_reset() -> Result<()> {
    let serving = passing_ack()
        .replace("kind=quiesced", "kind=status")
        .replace("state=off_confirmed", "state=serving")
        .replace("epoch=4", "epoch=0");
    let (master, slave) = TTYPort::pair().map_err(|error| anyhow!("TTY pair: {error}"))?;
    let responder = spawn_netcfg_responder(
        master,
        Box::leak(format!("{serving}\r\n").into_boxed_str()).as_bytes(),
    );
    let mut logger = Logger::new(None)?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "restored-known")?;
    runtime.pending_cycle = Some(PendingCycle {
        cycle: 4,
        boot: 17,
        off: parse_ack(&passing_ack(), &ack_regex()?)?,
        ble: None,
        ble_status: None,
    });
    runtime.ownership_known = Some(true);
    let mut reason = "health timeout".to_owned();
    assert!(runtime.revalidate_restored_ownership(&mut reason));
    drop(runtime);
    assert_eq!(responder.join().unwrap(), ["RADIOHANDOFF STATUS"]);

    let (master, slave) = TTYPort::pair().map_err(|error| anyhow!("TTY pair: {error}"))?;
    let reset_response = Box::leak(
        format!("Guru Meditation Error: Core 0 panic'ed\r\n{serving}\r\n").into_boxed_str(),
    );
    let responder = spawn_netcfg_responder(master, reset_response.as_bytes());
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "restored-reset")?;
    runtime.pending_cycle = Some(PendingCycle {
        cycle: 4,
        boot: 17,
        off: parse_ack(&passing_ack(), &ack_regex()?)?,
        ble: None,
        ble_status: None,
    });
    runtime.ownership_known = Some(true);
    let mut reason = "health timeout".to_owned();
    assert!(!runtime.revalidate_restored_ownership(&mut reason));
    assert_eq!(runtime.ownership_known, Some(false));
    assert!(reason.contains("revalidation failed"));
    drop(runtime);
    assert_eq!(responder.join().unwrap(), ["RADIOHANDOFF STATUS"]);
    Ok(())
}

#[test]
fn exact_restored_ack_marks_post_restore_service_failure_as_known() -> Result<()> {
    let (_master, slave) =
        TTYPort::pair().map_err(|error| anyhow!("TTYPort::pair failed: {error}"))?;
    let mut logger = Logger::new(None)?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "restored-transition")?;
    let restored_line = passing_ack()
        .replace("kind=quiesced", "kind=restored")
        .replace("state=off_confirmed", "state=serving");
    let restored = parse_ack(&restored_line, &ack_regex()?)?;

    runtime.ownership_known = Some(false);
    runtime.accept_restored_ownership(4, 17, &restored)?;

    assert_eq!(runtime.ownership_known, Some(true));
    assert_eq!(
        restore_failure_stage(runtime.ownership_known.unwrap_or(false)),
        "post_restore_service"
    );
    Ok(())
}

#[test]
fn stack_collection_uses_one_correlated_compact_command() -> Result<()> {
    let (master, slave) = TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))?;
    let responder = spawn_scripted_responder(
        master,
        vec![
            b"diagnostic headroom=16000STACK_STATUS cpu0=8192 touch=1024 tx_drop=7\r\n"
                .to_vec(),
            b"PSRAM feature_enabled=true state=Initialized total_bytes=1 used_bytes=2 free_bytes=3 peak_used_bytes=4 internal_free_bytes=17000 external_free_bytes=5 min_free_bytes=6 min_internal_free_bytes=16384 min_internal_alloc_charge_bytes=1700 min_internal_alloc_internal_required=true min_internal_alloc_charge_overflow=false min_internal_alloc_post_free_bytes=16384 min_internal_alloc_correlation_stable=true min_internal_alloc_wifi_rx_matched=true min_internal_alloc_released=true min_external_free_bytes=7 large_alloc_external_ok=0 large_alloc_internal_ok=0 large_alloc_fail=0 internal_probe_performed=false internal_probe_block_bytes=0 internal_probe_reserve_bytes=16384 internal_probe_free_before_bytes=17000 internal_probe_free_after_bytes=17000 internal_probe_stable=true\r\n".to_vec(),
        ],
    );
    let mut logger = Logger::new(None)?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "stack-status")?;
    collect_stack_metrics(&mut runtime)?;
    assert_eq!(runtime.cpu0_stack_headroom_min, Some(8_192));
    assert_eq!(runtime.touch_stack_headroom_min, Some(1_024));
    assert_eq!(runtime.uart_log_drops_final, Some(7));
    assert_eq!(runtime.serving_internal_free_min, Some(16_384));
    assert_eq!(runtime.serving_internal_min_alloc_charge, Some(1_700));
    assert_eq!(
        runtime.serving_internal_min_alloc_internal_required,
        Some(true)
    );
    drop(runtime);
    let commands = responder
        .join()
        .map_err(|_| anyhow!("stack status responder thread panicked"))?;
    assert_eq!(commands, ["STACKSTATUS", "ALLOCSTATUS"]);
    Ok(())
}

#[test]
fn initial_identity_captures_drop_baseline_before_owner_status() -> Result<()> {
    let (master, slave) =
        TTYPort::pair().map_err(|error| anyhow!("TTYPort::pair failed: {error}"))?;
    let status = passing_ack()
        .replace("kind=quiesced", "kind=status")
        .replace("state=off_confirmed", "state=serving")
        .replace("epoch=4", "epoch=0");
    let responder = spawn_scripted_responder(
        master,
        vec![
            b"STACK_STATUS cpu0=8192 touch=1024 tx_drop=5\r\n".to_vec(),
            format!("{status}\r\n").into_bytes(),
        ],
    );
    let mut logger = Logger::new(None)?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "initial-identity")?;

    runtime.capture_initial_owner_identity()?;
    assert_eq!(runtime.uart_log_drops_baseline, Some(5));
    assert_eq!(runtime.boot_generation, Some(17));
    drop(runtime);
    let commands = responder
        .join()
        .map_err(|_| anyhow!("identity responder thread panicked"))?;
    assert_eq!(commands, ["STACKSTATUS", "RADIOHANDOFF STATUS"]);
    Ok(())
}

#[test]
fn stack_collection_rejects_a_panic_before_the_correlated_response() -> Result<()> {
    let (master, slave) = TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))?;
    let responder = spawn_netcfg_responder(
        master,
        b"Guru Meditation Error: Core 0 panic'ed\r\nSTACK_STATUS cpu0=8192 touch=1024 tx_drop=0\r\n",
    );
    let mut logger = Logger::new(None)?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "stack-panic")?;
    let error = collect_stack_metrics(&mut runtime)
        .expect_err("panic before stack status must fail collection");
    assert!(error.to_string().contains("panic/reset"));
    drop(runtime);
    let commands = responder
        .join()
        .map_err(|_| anyhow!("stack panic responder thread panicked"))?;
    assert_eq!(commands, ["STACKSTATUS"]);
    Ok(())
}

#[test]
fn below_floor_values_are_persisted_before_the_real_assertion_fails() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let report_path = temp.path().join("phase1s.json");
    let log_path = temp.path().join("hostctl.jsonl");
    let (_master, slave) = TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))?;
    let mut logger = Logger::new(Some(log_path.clone()))?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "stack-report")?;
    runtime.report_path = report_path.clone();
    runtime.samples = (1..=20).map(sample).collect();
    runtime.ble_samples = (1..=20).map(ble_sample).collect();
    runtime.cpu0_stack_headroom_min = Some(8_191);
    runtime.touch_stack_headroom_min = Some(1_024);
    runtime.serving_internal_free_min = Some(16_384);
    runtime.uart_log_drops_baseline = Some(5);
    runtime.uart_log_drops_final = Some(5);

    runtime.invoke("write_report", &json!({}), &mut json!({}))?;
    let report: Value = serde_json::from_slice(&std::fs::read(&report_path)?)?;
    assert_eq!(report["cpu0_stack_headroom_min"], 8_191);
    assert_eq!(report["touch_stack_headroom_min"], 1_024);
    assert_eq!(report["uart_log_drops_baseline"], 5);
    assert_eq!(report["uart_log_drops_final"], 5);
    assert_eq!(report["uart_log_drops_during_gate"], 0);
    assert_eq!(report["ble_samples"][0]["queue_task_cancelled"], 1);
    assert_eq!(report["ble_samples"][0]["queue_operation_balance_error"], 0);
    assert_eq!(report["ble_samples"][0]["queue_task_live"], 0);
    assert_eq!(report["ble_samples"][0]["queue_task_faults"], 0);
    assert_eq!(report["ble_samples"][0]["queue_operation_registry_full"], 0);
    assert_eq!(report["gate_passed"], false);
    assert!(report["violations"][0]
        .as_str()
        .expect("violation")
        .contains("CPU0 stack gate failed"));
    let error = runtime
        .invoke("assert_stack_floors", &json!({}), &mut json!({}))
        .expect_err("below-floor CPU0 value must fail after report persistence");
    assert!(error.to_string().contains("CPU0 stack gate failed"));
    drop(runtime);
    drop(logger);
    assert!(!std::fs::read_to_string(log_path)?.contains("Wi-Fi-only gate passed"));
    Ok(())
}

#[test]
fn in_window_uart_drop_is_reported_before_the_real_gate_fails() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let report_path = temp.path().join("phase1s-drop.json");
    let log_path = temp.path().join("hostctl-drop.jsonl");
    let (_master, slave) = TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))?;
    let mut logger = Logger::new(Some(log_path.clone()))?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "uart-drop-report")?;
    runtime.report_path = report_path.clone();
    runtime.samples = (1..=20).map(sample).collect();
    runtime.ble_samples = (1..=20).map(ble_sample).collect();
    runtime.cpu0_stack_headroom_min = Some(8_192);
    runtime.touch_stack_headroom_min = Some(1_024);
    runtime.serving_internal_free_min = Some(16_384);
    runtime.uart_log_drops_baseline = Some(5);
    runtime.uart_log_drops_final = Some(6);

    runtime.invoke("write_report", &json!({}), &mut json!({}))?;
    let report: Value = serde_json::from_slice(&std::fs::read(&report_path)?)?;
    assert_eq!(report["uart_log_drops_during_gate"], 1);
    assert_eq!(report["gate_passed"], false);
    assert!(report["violations"][0]
        .as_str()
        .expect("violation")
        .contains("UART diagnostic drop gate failed"));
    let error = runtime
        .invoke("assert_stack_floors", &json!({}), &mut json!({}))
        .expect_err("an in-window UART drop must fail after report persistence");
    assert!(error
        .to_string()
        .contains("UART diagnostic drop gate failed"));
    drop(runtime);
    drop(logger);
    assert!(!std::fs::read_to_string(log_path)?.contains("Wi-Fi-only gate passed"));
    Ok(())
}

#[test]
fn passing_final_gate_report_has_an_explicit_true_verdict() -> Result<()> {
    let (_master, slave) =
        TTYPort::pair().map_err(|error| anyhow!("TTYPort::pair failed: {error}"))?;
    let mut logger = Logger::new(None)?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "passing-verdict")?;
    runtime.samples = (1..=20).map(sample).collect();
    runtime.ble_samples = (1..=20).map(ble_sample).collect();
    runtime.cpu0_stack_headroom_min = Some(8_192);
    runtime.touch_stack_headroom_min = Some(1_024);
    runtime.serving_internal_free_min = Some(16_384);
    runtime.uart_log_drops_baseline = Some(5);
    runtime.uart_log_drops_final = Some(5);

    let report = serde_json::to_value(runtime.finish_report()?)?;
    assert_eq!(report["gate_passed"], true);
    assert_eq!(report["violations"], json!([]));
    assert_eq!(report["uart_log_drops_during_gate"], 0);
    Ok(())
}

#[test]
fn serving_low_water_is_persisted_before_the_final_gate_fails() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let report_path = temp.path().join("phase1s-serving-floor.json");
    let (_master, slave) =
        TTYPort::pair().map_err(|error| anyhow!("TTYPort::pair failed: {error}"))?;
    let mut logger = Logger::new(None)?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "serving-floor-verdict")?;
    runtime.report_path = report_path.clone();
    runtime.samples = (1..=20).map(sample).collect();
    runtime.ble_samples = (1..=20).map(ble_sample).collect();
    runtime.cpu0_stack_headroom_min = Some(8_192);
    runtime.touch_stack_headroom_min = Some(1_024);
    runtime.serving_internal_free_min = Some(16_383);
    runtime.serving_internal_min_alloc_wifi_rx_matched = Some(true);
    runtime.serving_internal_min_alloc_correlation_stable = Some(true);
    runtime.serving_internal_min_alloc_released = Some(true);
    runtime.uart_log_drops_baseline = Some(5);
    runtime.uart_log_drops_final = Some(5);

    runtime.invoke("write_report", &json!({}), &mut json!({}))?;
    let report: Value = serde_json::from_slice(&std::fs::read(&report_path)?)?;
    assert_eq!(report["minimum_serving_internal_free_bytes"], 16_383);
    assert_eq!(report["minimum_serving_internal_alloc_charge_bytes"], 1_700);
    assert_eq!(
        report["minimum_serving_internal_alloc_internal_required"],
        true
    );
    assert_eq!(
        report["minimum_serving_internal_alloc_wifi_rx_matched"],
        true
    );
    assert_eq!(
        report["minimum_serving_internal_alloc_correlation_stable"],
        true
    );
    assert_eq!(report["minimum_serving_internal_alloc_released"], true);
    assert_eq!(report["gate_passed"], false);
    assert!(report["violations"][0]
        .as_str()
        .expect("violation")
        .contains("serving internal-free floor failed"));
    let error = runtime
        .invoke("assert_stack_floors", &json!({}), &mut json!({}))
        .expect_err("serving low-water must fail after report persistence");
    assert!(error
        .to_string()
        .contains("serving internal-free floor failed"));
    Ok(())
}

#[test]
fn excessive_drift_is_persisted_before_the_shared_final_gate_fails() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let report_path = temp.path().join("phase1s-drift.json");
    let (_master, slave) =
        TTYPort::pair().map_err(|error| anyhow!("TTYPort::pair failed: {error}"))?;
    let mut logger = Logger::new(None)?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "drift-verdict")?;
    runtime.report_path = report_path.clone();
    runtime.samples = (1..=20).map(sample).collect();
    runtime.ble_samples = (1..=20).map(ble_sample).collect();
    runtime.samples[1].internal_free += 1_025;
    runtime.cpu0_stack_headroom_min = Some(8_192);
    runtime.touch_stack_headroom_min = Some(1_024);
    runtime.serving_internal_free_min = Some(16_384);
    runtime.uart_log_drops_baseline = Some(5);
    runtime.uart_log_drops_final = Some(5);

    runtime.invoke("write_report", &json!({}), &mut json!({}))?;
    let report: Value = serde_json::from_slice(&std::fs::read(&report_path)?)?;
    assert_eq!(report["gate_passed"], false);
    assert_eq!(report["post_warmup_free_drift"], 1_025);
    assert!(report["violations"][0]
        .as_str()
        .expect("violation")
        .contains("post-warm-up drift exceeded"));
    let error = runtime
        .invoke("assert_stack_floors", &json!({}), &mut json!({}))
        .expect_err("the shared final verdict must reject drift");
    assert!(error.to_string().contains("post-warm-up drift exceeded"));
    Ok(())
}

#[test]
fn workflow_binds_identity_and_each_provisioning_step_before_cycles() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1s.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    assert_eq!(workflow.document.name, "ble-phase1s");
    let raw = std::fs::read_to_string(&path).expect("workflow source");
    assert!(raw.contains("in: \".cycle_count\""));

    let mut runtime = RecordingRuntime::default();
    execute_workflow(&workflow, &mut runtime, &json!({})).expect("workflow executes");
    assert_eq!(
        runtime.calls,
        [
            "await_ready",
            "capture_owner_identity",
            "close_listener",
            "start_network",
            "await_sd_ready",
            "drain_serial_backlog",
            "apply_network_config",
            "stop_network",
            "await_network_idle",
            "start_network",
            "await_network_ready",
            "verify_network_config",
            "open_listener",
            "await_listener_ready",
            "verify_provisioning",
            "init_run",
            "prepare_off_window",
            "run_ble_window",
            "restore_and_complete_cycle",
            "prepare_off_window",
            "run_ble_window",
            "restore_and_complete_cycle",
            "collect_stack_metrics",
            "write_report",
            "assert_stack_floors",
        ],
        "identity must precede provisioning and no cycle may start before both complete"
    );
}

#[test]
fn a_stack_floor_failure_occurs_after_the_evidence_report_action() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1s.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    let mut runtime = RecordingRuntime {
        fail_action: Some("assert_stack_floors".to_owned()),
        ..RecordingRuntime::default()
    };

    let error = execute_workflow(&workflow, &mut runtime, &json!({}))
        .expect_err("stack floor must fail after report persistence");
    assert!(error.to_string().contains("assert_stack_floors"));
    let report = runtime
        .calls
        .iter()
        .position(|call| call == "write_report")
        .expect("report action");
    let assertion = runtime
        .calls
        .iter()
        .position(|call| call == "assert_stack_floors")
        .expect("assert action");
    assert!(report < assertion);
}

#[test]
fn final_metrics_failure_writes_failure_report_before_failing() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1s.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    let mut runtime = RecordingRuntime {
        fail_action: Some("fail_final_metrics".to_owned()),
        metrics_outcome: Some("failed".to_owned()),
        ..RecordingRuntime::default()
    };

    execute_workflow(&workflow, &mut runtime, &json!({}))
        .expect_err("metrics failure must fail after retaining evidence");
    assert_eq!(
        &runtime.calls[runtime.calls.len() - 3..],
        [
            "collect_stack_metrics",
            "write_failure_report",
            "fail_final_metrics",
        ]
    );
    assert!(!runtime.calls.iter().any(|call| call == "write_report"));
}

#[test]
fn workflow_restores_known_ble_failure_before_writing_failure_evidence() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1s.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    let mut runtime = RecordingRuntime {
        fail_action: Some("fail_ble_window".to_owned()),
        ble_outcome: Some("known_failed".to_owned()),
        ..RecordingRuntime::default()
    };

    execute_workflow(&workflow, &mut runtime, &json!({}))
        .expect_err("known BLE failure must fail after restoration and report");
    let tail = &runtime.calls[runtime.calls.len() - 4..];
    assert_eq!(
        tail,
        [
            "run_ble_window",
            "restore_after_known_failure",
            "write_failure_report",
            "fail_ble_window",
        ]
        .as_slice()
    );
}

#[test]
fn workflow_never_restores_when_ble_ownership_is_unknown() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1s.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    let mut runtime = RecordingRuntime {
        fail_action: Some("fail_ble_window".to_owned()),
        ble_outcome: Some("ownership_unknown".to_owned()),
        ..RecordingRuntime::default()
    };

    execute_workflow(&workflow, &mut runtime, &json!({}))
        .expect_err("unknown BLE ownership must fail after report without restoration");
    assert!(!runtime
        .calls
        .iter()
        .any(|call| call == "restore_after_known_failure"));
    assert_eq!(
        &runtime.calls[runtime.calls.len() - 3..],
        ["run_ble_window", "write_failure_report", "fail_ble_window"]
    );
}

#[test]
fn workflow_restores_exact_off_settlement_failure_before_report() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1s.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    let mut runtime = RecordingRuntime {
        fail_action: Some("fail_ble_window".to_owned()),
        handoff_outcome: Some("known_off_failed".to_owned()),
        ..RecordingRuntime::default()
    };

    execute_workflow(&workflow, &mut runtime, &json!({}))
        .expect_err("known off failure must restore and persist evidence");
    assert_eq!(
        &runtime.calls[runtime.calls.len() - 4..],
        [
            "prepare_off_window",
            "restore_after_known_failure",
            "write_failure_report",
            "fail_ble_window",
        ]
    );
}

#[test]
fn workflow_reports_known_serving_rejection_without_a_second_restore() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1s.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    let mut runtime = RecordingRuntime {
        fail_action: Some("fail_ble_window".to_owned()),
        handoff_outcome: Some("known_serving_failed".to_owned()),
        ..RecordingRuntime::default()
    };

    execute_workflow(&workflow, &mut runtime, &json!({}))
        .expect_err("known serving rejection must persist evidence and fail");
    assert!(!runtime
        .calls
        .iter()
        .any(|call| call == "restore_after_known_failure"));
    assert_eq!(
        &runtime.calls[runtime.calls.len() - 3..],
        [
            "prepare_off_window",
            "write_failure_report",
            "fail_ble_window"
        ]
    );
}

#[test]
fn only_exact_correlated_serving_rejection_proves_known_ownership() -> Result<()> {
    let regex = ack_regex()?;
    let line = "RADIO_HANDOFF_ACK kind=rejected state=serving reason=resource_floor boot=17 epoch=7 internal_free=25168 block_above_reserve=0 probe_before=25168 probe_after=25168 probe_reserve=16384 http=0 sd_roundtrip=0 sd_session=0 callbacks=0 queues=2 source_active=true callback_admission=true late_callbacks=0 queue_late=0 queue_unknown=0 queue_reclaim_fail=0 queue_corruption=0 queue_contention=0 stable=true";
    let ack = parse_ack(line, &regex)?;
    assert!(is_known_serving_rejection(7, 17, &ack));
    assert!(!is_known_serving_rejection(6, 17, &ack));
    assert!(!is_known_serving_rejection(7, 18, &ack));

    let faulted = parse_ack(&line.replace("state=serving", "state=faulted"), &regex)?;
    assert!(!is_known_serving_rejection(7, 17, &faulted));
    let busy = parse_ack(
        &line.replace("reason=resource_floor", "reason=busy"),
        &regex,
    )?;
    assert!(!is_known_serving_rejection(7, 17, &busy));
    Ok(())
}

#[test]
fn workflow_persists_restore_failure_before_failing() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1s.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    let mut runtime = RecordingRuntime {
        fail_action: Some("fail_ble_window".to_owned()),
        restore_outcome: Some("failed".to_owned()),
        ..RecordingRuntime::default()
    };

    execute_workflow(&workflow, &mut runtime, &json!({}))
        .expect_err("restore failure must persist evidence before failing");
    assert_eq!(
        &runtime.calls[runtime.calls.len() - 4..],
        [
            "run_ble_window",
            "restore_and_complete_cycle",
            "write_failure_report",
            "fail_ble_window",
        ]
    );
}

#[test]
fn busy_ble_rejection_is_never_safe_to_restore() {
    assert_eq!(rejected_ble_outcome("busy"), ("ownership_unknown", false));
    assert_eq!(
        rejected_ble_outcome("ownership_unknown"),
        ("ownership_unknown", false)
    );
    assert_eq!(rejected_ble_outcome("consumed"), ("known_failed", true));
}

#[test]
fn failure_report_retains_pending_off_and_raw_ble_status() -> Result<()> {
    let (master, slave) =
        TTYPort::pair().map_err(|error| anyhow!("TTYPort::pair failed: {error}"))?;
    drop(master);
    let mut logger = Logger::new(None)?;
    let mut runtime = netcfg_test_runtime(&mut logger, slave, "ble-failure-report")?;
    let off = parse_ack(&passing_ack(), &ack_regex()?)?;
    let status_re = ble_window_status_regex()?;
    let status = parse_ble_window_status(&passing_ble_status(), &status_re)?;
    runtime.failure_stage = Some("ble_window".to_owned());
    runtime.failure_reason = Some("resource_floor".to_owned());
    runtime.ownership_known = Some(true);
    runtime.pending_cycle = Some(PendingCycle {
        cycle: 4,
        boot: 17,
        off,
        ble: None,
        ble_status: Some(status),
    });

    let report = serde_json::to_value(runtime.finish_failure_report())?;
    assert_eq!(report["gate_passed"], false);
    assert_eq!(report["failure_stage"], "ble_window");
    assert_eq!(report["failure_reason"], "resource_floor");
    assert_eq!(report["ownership_known"], true);
    assert_eq!(report["pending_off"]["epoch"], 4);
    assert_eq!(report["pending_ble_status"]["active_free"], 20_000);
    assert_eq!(report["pending_ble_status"]["queue_task_cancelled"], 1);
    assert_eq!(
        report["pending_ble_status"]["queue_operation_balance_error"],
        0
    );
    assert_eq!(report["pending_ble_status"]["queue_task_live"], 0);
    assert_eq!(report["pending_ble_status"]["queue_task_faults"], 0);
    assert_eq!(
        report["pending_ble_status"]["queue_operation_registry_full"],
        0
    );
    assert_eq!(report["cpu0_stack_headroom_min"], Value::Null);
    Ok(())
}

#[test]
fn phase1s_upload_probe_stays_under_the_firmware_upload_root() {
    let source = include_str!("mod.rs");
    assert!(
        source.contains(r#"dst_root: "/assets/phase1s""#),
        "Phase 1S probes must use the firmware's /assets upload root"
    );
    assert!(
        !source.contains(r#"dst_root: "/phase1s""#),
        "the firmware rejects upload destinations outside /assets"
    );
}

#[test]
fn startup_sd_probe_is_correlated_and_must_be_ready_before_netcfg() -> Result<()> {
    for (case, response, succeeds) in [
        (
            "ready",
            b"SDWAIT DONE target=id wait_id=0 id=0 op=probe status=ok code=ok attempts=2 dur_ms=2915\r\n"
                .as_slice(),
            true,
        ),
        (
            "wrong-id",
            b"SDWAIT DONE target=id wait_id=0 id=1 op=probe status=ok code=ok attempts=1 dur_ms=1\r\n"
                .as_slice(),
            false,
        ),
        (
            "failed",
            b"SDWAIT DONE target=id wait_id=0 id=0 op=probe status=error code=init_failed attempts=3 dur_ms=3833\r\n"
                .as_slice(),
            false,
        ),
    ] {
        let (master, slave) =
            TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))?;
        let responder = spawn_netcfg_responder(master, response);
        let mut logger = Logger::new(None)?;
        let mut runtime = netcfg_test_runtime(&mut logger, slave, case)?;
        let result = runtime.await_sd_ready();
        drop(runtime);
        let commands = responder
            .join()
            .map_err(|_| anyhow!("SD readiness responder thread panicked"))?;
        assert_eq!(commands, ["SDWAIT 0 15000"]);
        assert_eq!(result.is_ok(), succeeds, "{case}: {result:?}");
    }
    Ok(())
}

#[test]
fn netcfg_set_is_single_shot_and_requires_an_unambiguous_ack() -> Result<()> {
    assert_eq!(NETCFG_TX_CHUNK_BYTES, 32);
    assert_eq!(NETCFG_TX_SETTLE_MS, 60);
    for (case, response, expected_error) in [
        ("ok", b"NET OK op=config_set\r\n".as_slice(), None),
        (
            "wrong-op",
            b"NET OK op=start\r\n".as_slice(),
            Some("no unambiguous acknowledgement"),
        ),
        (
            "persist-failed",
            b"NET ERR reason=persist_failed code=operation_failed\r\n".as_slice(),
            Some("persist_failed"),
        ),
    ] {
        let (master, slave) =
            TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))?;
        let responder = spawn_netcfg_responder(master, response);
        let mut logger = Logger::new(None)?;
        let mut runtime = netcfg_test_runtime(&mut logger, slave, case)?;
        let result = runtime.apply_network_config_once();
        drop(runtime);
        let commands = responder
            .join()
            .map_err(|_| anyhow!("NETCFG responder thread panicked"))?;

        assert_eq!(commands.len(), 1, "{case}: NETCFG SET must never retry");
        assert!(
            commands[0].len() <= 96,
            "{case}: command exceeds safe line bound"
        );
        assert!(
            !commands[0].contains("connect_timeout_ms"),
            "{case}: default policy must be supplied by firmware defaults, not a long UART line"
        );
        assert!(
            commands[0].starts_with("NETCFG SET {") && commands[0].contains("test-ssid"),
            "{case}: unexpected command: {}",
            commands[0]
        );
        match expected_error {
            None => result.with_context(|| format!("{case}: expected success"))?,
            Some(expected) => {
                let error = result.expect_err("NETCFG acknowledgement must be rejected");
                assert!(
                    error.to_string().contains(expected),
                    "{case}: unexpected error: {error}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn compact_provisioning_rejects_a_nondefault_policy_before_uart_mutation() -> Result<()> {
    let mut policy = NetPolicy::default();
    policy.connect_timeout_ms += 1;
    let error = build_phase1s_netcfg_command("test-ssid", "test-password", policy)
        .expect_err("nondefault policy must not use compact provisioning");
    assert!(error
        .to_string()
        .contains("requires the default network policy"));
    Ok(())
}

#[test]
fn compact_provisioning_rejects_unrepresentable_credentials_before_serial_open() {
    for (ssid, password) in [
        ("bad\"ssid", "password"),
        ("bad\\ssid", "password"),
        ("bad\nssid", "password"),
        ("ssid", "bad\"password"),
        ("ssid", "bad\\password"),
        ("ssid", "bad\npassword"),
        (" leading-ssid", "password"),
        ("trailing-ssid ", "password"),
        ("ssid", " leading-password"),
        ("ssid", "trailing-password "),
    ] {
        assert!(
            build_phase1s_netcfg_command(ssid, password, NetPolicy::default()).is_err(),
            "unsupported credentials must fail before serial open"
        );
    }
    assert!(
        build_phase1s_netcfg_command(&"s".repeat(33), "password", NetPolicy::default()).is_err()
    );
    assert!(build_phase1s_netcfg_command("ssid", &"p".repeat(65), NetPolicy::default()).is_err());

    let source = include_str!("setup.rs");
    let validation = source
        .find("build_phase1s_netcfg_command(&ssid, &password, policy)?")
        .expect("preflight command validation");
    let serial_open = source.find("SerialConsole::open").expect("serial open");
    assert!(validation < serial_open);
}

#[test]
fn acceptance_requires_twenty_cycles_and_board_identity() {
    for cycles in [0, 1, 19] {
        assert!(
            validate_gate_options("board-a", cycles).is_err(),
            "{cycles}"
        );
    }
    assert!(validate_gate_options("", 20).is_err());
    validate_gate_options("board-a", 20).expect("twenty-cycle gate");
}

#[test]
fn report_requires_the_complete_consecutive_cycle_sequence() {
    let nineteen: Vec<_> = (1..=19).map(sample).collect();
    assert!(validate_completed_samples(&nineteen, 20).is_err());

    let twenty: Vec<_> = (1..=20).map(sample).collect();
    validate_completed_samples(&twenty, 20).expect("complete gate");
    assert!(validate_completed_samples(&twenty, 21).is_err());

    let mut skipped = twenty;
    skipped[9].cycle = 11;
    assert!(validate_completed_samples(&skipped, 20).is_err());
}
