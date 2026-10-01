use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::line_checks::{
    headroom_minimum, metrics_listener, mountain_ui_status_values, snooped_listener,
    stack_status_values, verify_cycle_psram, verify_resident_timing,
};
use super::markers::{marker_pattern, MARKER_TABLE};
use super::report::{
    all_required_pass, assemble_report, caught_message, checkpoint_satisfied, parse_psram_line,
    report_path_for, CheckpointState,
};
use super::{reentry_required_checkpoints, AssetResidencyRuntime, REQUIRED_CHECKPOINTS};
use crate::logging::Logger;
use crate::scenarios::{execute_workflow, load_workflow, WorkflowRuntime};
use crate::serial_console::{mock::MockPort, SerialConsole};

#[test]
fn psram_parser_keeps_every_field() {
    let sample = parse_psram_line(
        "ambient_ready",
        "PSRAM feature_enabled=true state=ready total_bytes=8388608 used_bytes=1024 free_bytes=8387584 custom_field=7",
    )
    .expect("full line parses");
    assert_eq!(sample.label, "ambient_ready");
    assert_eq!(sample.fields["feature_enabled"], "true");
    assert_eq!(sample.fields["total_bytes"], "8388608");
    assert_eq!(sample.fields["custom_field"], "7");
    assert!(sample.raw_line.starts_with("PSRAM "));
}

#[test]
fn report_path_never_overwrites_a_json_capture() {
    assert_eq!(
        report_path_for(PathBuf::from("logs/watch.log").as_path()),
        PathBuf::from("logs/watch.json")
    );
    assert_eq!(
        report_path_for(PathBuf::from("logs/watch.json").as_path()),
        PathBuf::from("logs/watch.report.json")
    );
}

#[test]
fn psram_parser_accepts_partial_lines() {
    let sample = parse_psram_line("clock_first", "PSRAM free_bytes=12").expect("partial parses");
    assert_eq!(sample.fields.len(), 1);
    assert_eq!(sample.fields["free_bytes"], "12");
}

#[test]
fn psram_parser_rejects_garbage() {
    assert!(parse_psram_line("x", "METRICS free_bytes=1").is_err());
    assert!(parse_psram_line("x", "PSRAMX free_bytes=1").is_err());
    assert!(parse_psram_line("x", "").is_err());
}

#[test]
fn report_preserves_labels_and_fills_missing() {
    let mut checkpoints = HashMap::new();
    checkpoints.insert(
        "ambient_assets_adopted".to_string(),
        CheckpointState {
            status: "pass".to_string(),
            elapsed_ms: Some(11),
            marker_line: Some("AMBIENT_ASSETS status=adopted".to_string()),
            psram: None,
        },
    );
    let report = assemble_report(
        "feat 1.0",
        "build #42",
        "2026-09-24T00:00:00Z",
        &REQUIRED_CHECKPOINTS
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>(),
        &checkpoints,
        &["STATE phase=idle upload=off ready=true".to_string()],
        None,
    );
    assert_eq!(report.feature_label, "feat 1.0");
    assert_eq!(report.build_label, "build #42");
    assert_eq!(
        report
            .checkpoints
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        REQUIRED_CHECKPOINTS
    );
    let adopted = report
        .checkpoints
        .iter()
        .find(|entry| entry.name == "ambient_assets_adopted")
        .expect("ambient_assets_adopted entry present");
    assert_eq!(adopted.status, "pass");
    assert_eq!(adopted.elapsed_ms, Some(11));
    let ready = report
        .checkpoints
        .iter()
        .find(|entry| entry.name == "ready")
        .expect("ready entry present");
    assert_eq!(ready.status, "missing");
    assert!(ready.marker_line.is_none());
    assert!(ready.psram.is_none());
    for entry in report
        .checkpoints
        .iter()
        .filter(|entry| entry.name != "ambient_assets_adopted" && entry.name != "ready")
    {
        assert_eq!(entry.status, "missing", "{}", entry.name);
        assert!(entry.marker_line.is_none());
        assert!(entry.psram.is_none());
    }
    assert!(!all_required_pass(
        &checkpoints,
        &REQUIRED_CHECKPOINTS
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>()
    ));
    assert!(caught_message(&json!({})).is_none());
    assert_eq!(
        caught_message(&json!({"measurement_error": {"message": "boom", "attempt": 1}})),
        Some("boom".to_string())
    );
}

#[test]
fn marker_table_matches_device_contract() {
    let table = MARKER_TABLE.iter().cloned().collect::<HashMap<_, _>>();
    assert_eq!(
        table["ambient_assets_adopted"],
        r"^AMBIENT_ASSETS status=adopted(\s|$)"
    );
    assert_eq!(
        table["mountain_validated"],
        r"^MOUNTAIN_STREAM status=validated(\s|$)"
    );
    assert_eq!(
        table["mountain_rendered"],
        r"^MOUNTAIN_STREAM status=rendered(\s|$)"
    );
    assert_eq!(
        table["mountain_adopted"],
        r"^AMBIENT_MOUNTAIN status=adopted(\s|$)"
    );
    assert_eq!(
        table["mountain_composed"],
        r"^AMBIENT_MOUNTAIN status=composed(\s|$)"
    );
    assert_eq!(
        table["clock_entered"],
        r"^CLOCK_SCREEN status=entered(\s|$)"
    );
    assert_eq!(
        table["clock_assets_ready"],
        r"^CLOCK_ASSETS status=(complete|borrowed)(\s|$)"
    );
    assert_eq!(
        table["clock_frame_complete"],
        r"^CLOCK_FRAME status=complete(\s|$)"
    );
    assert_eq!(table["clock_profile"], r"^CLOCK_PROFILE(\s|$)");
    assert_eq!(
        table["clock_assets_released"],
        r"^CLOCK_ASSETS status=released(\s|$)"
    );
    for (key, _) in MARKER_TABLE {
        marker_pattern(key).expect("known key compiles");
    }
    assert!(marker_pattern("nope").is_err());
    let adopted = marker_pattern("ambient_assets_adopted").expect("pattern");
    assert!(adopted.is_match("AMBIENT_ASSETS status=adopted at_ms=3"));
    assert!(!adopted.is_match("AMBIENT_ASSETS status=composed"));
    let validated = marker_pattern("mountain_validated").expect("pattern");
    assert!(validated
        .is_match("MOUNTAIN_STREAM status=validated generation=2 duration_ms=9 eligible=7"));
    assert!(!validated.is_match("MOUNTAIN_STREAM status=validation_reused generation=2"));
    assert!(!validated
        .is_match("MOUNTAIN_STREAM status=rendered percent=30 rows=1 duration_ms=2 max_row_ms=1"));
    let rendered = marker_pattern("mountain_rendered").expect("pattern");
    assert!(rendered
        .is_match("MOUNTAIN_STREAM status=rendered percent=30 rows=1 duration_ms=2 max_row_ms=1"));
    assert!(!rendered
        .is_match("MOUNTAIN_STREAM status=validated generation=2 duration_ms=9 eligible=7"));
    let clock_assets_ready = marker_pattern("clock_assets_ready").expect("pattern");
    assert!(clock_assets_ready.is_match("CLOCK_ASSETS status=complete id=1"));
    assert!(clock_assets_ready.is_match("CLOCK_ASSETS status=borrowed target_none=false"));
    assert!(!clock_assets_ready.is_match("CLOCK_FRAME status=complete"));
    assert!(!clock_assets_ready.is_match("CLOCK_ASSETS status=requested id=1"));
    assert!(!clock_assets_ready.is_match("CLOCK_ASSETS status=read_error id=1 attempt=1"));
    assert!(marker_pattern("mountain_cache_validated")
        .unwrap()
        .is_match("MOUNTAIN_CACHE status=validated bytes=665839 chunks=21"));
    assert!(marker_pattern("mountain_released")
        .unwrap()
        .is_match("MOUNTAIN_CACHE status=released reason=screen_exit bytes=665839"));
}

#[test]
fn reentry_timing_and_psram_checks_reject_fallback_or_allocation_failure() {
    let timing = "MOUNTAIN_TIMING id=3 total_us=3177788 histogram_reads=0 render_reads=0 resident_bytes=665839 ok=true";
    assert!(verify_resident_timing(timing).is_ok());
    assert!(
        verify_resident_timing(&timing.replace("render_reads=0", "render_reads=1345")).is_err()
    );
    assert!(
        verify_resident_timing(&timing.replace("resident_bytes=665839", "resident_bytes=0"))
            .is_err()
    );
    let sample = parse_psram_line(
        "clock_first",
        "PSRAM external_free_bytes=270260 internal_free_bytes=27648 min_external_free_bytes=270260 min_internal_free_bytes=25748 large_alloc_fail=0",
    )
    .expect("sample parses");
    assert!(verify_cycle_psram(&sample).is_ok());
    assert!(verify_cycle_psram(
        &parse_psram_line(
            "failed",
            &sample
                .raw_line
                .replace("large_alloc_fail=0", "large_alloc_fail=1")
        )
        .unwrap()
    )
    .is_err());
}

#[test]
fn reentry_workflow_repeats_same_boot_route_and_preserves_per_cycle_checkpoints() -> Result<()> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/asset-residency-reentry.sw.yaml");
    let workflow = load_workflow(&path)?;
    assert_eq!(workflow.document.name, "asset-residency-reentry");
    let yaml = std::fs::read_to_string(path)?;
    assert!(yaml.contains("in: \".reentry_cycles\""));
    assert!(yaml.contains("target: \"AMBIENT_VIEW\""));
    assert!(yaml.contains("target: \"ANALOG_CLOCK\""));
    let required = reentry_required_checkpoints(2);
    assert_eq!(required.len(), 2 * super::REENTRY_CHECKPOINTS.len());
    assert_eq!(required.first().unwrap(), "cycle_01_ambient_entered");
    assert_eq!(required.last().unwrap(), "cycle_02_no_fault");

    let mut runtime = RecordingRuntime::default();
    execute_workflow(&workflow, &mut runtime, &json!({"reentry_cycles": 2}))?;
    let count = |name: &str| {
        runtime
            .calls
            .iter()
            .filter(|call| call.as_str() == name)
            .count()
    };
    assert_eq!(count("begin_cycle"), 2);
    assert_eq!(count("ui_command"), 10);
    assert_eq!(count("record_mountain_timing"), 2);
    assert_eq!(count("verify_mountain_ui_status"), 2);
    assert_eq!(count("assert_no_fault"), 2);
    assert_eq!(count("state_command"), 4);
    assert_eq!(count("cleanup_upload_off"), 1);
    assert_eq!(count("finalize_report"), 1);
    assert!(!runtime.calls.iter().any(|call| call == "fail_evidence"));

    let mut checker = silent_test_runtime(MockPort::new());
    checker.invoke("begin_cycle", &json!({"index": 0}), &mut json!({}))?;
    checker.record_marker("mountain_timing", "pass", None);
    checker.invoke("begin_cycle", &json!({"index": 1}), &mut json!({}))?;
    checker.record_marker("mountain_timing", "pass", None);
    assert!(checker.checkpoints.contains_key("cycle_01_mountain_timing"));
    assert!(checker.checkpoints.contains_key("cycle_02_mountain_timing"));
    Ok(())
}

#[test]
fn mountain_ui_status_parser_requires_correlated_fields() {
    assert_eq!(
        mountain_ui_status_values("MOUNTAIN_STATUS adopted=1 composed=1 tx_drop=42"),
        Some((true, true, 42))
    );
    assert_eq!(
        mountain_ui_status_values("MOUNTAIN_STATUS adopted=1 composed=0 tx_drop=0"),
        Some((true, false, 0))
    );
    assert_eq!(mountain_ui_status_values("MOUNTAIN_STATUS adopted=1"), None);
}

#[test]
fn baseline_composition_uses_correlated_status() {
    let status = b"MOUNTAIN_STATUS adopted=1 composed=1 tx_drop=4\r\n";
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[status]));
    runtime
        .invoke(
            "verify_mountain_ui_status",
            &json!({"checkpoint": "mountain_composed"}),
            &mut json!({}),
        )
        .expect("correlated status proves composition");
    assert_eq!(runtime.checkpoints["mountain_composed"].status, "pass");
}

#[test]
fn headroom_minimum_parses_diag_lines() {
    assert_eq!(
        headroom_minimum("stack_diag: tag=minimum headroom=8192"),
        Some("8192".to_string())
    );
    assert_eq!(
        headroom_minimum("touch_core_stack_diag: tag=minimum headroom=512"),
        Some("512".to_string())
    );
    assert_eq!(headroom_minimum("METRICS NET_ACCEPT ok=1"), None);
    assert_eq!(
        headroom_minimum("stack_diag: tag=minimum headroom=abc"),
        None
    );
    assert_eq!(headroom_minimum(""), None);
}

#[test]
fn stack_headroom_watch_polls_metrics_then_cleans_up() -> Result<()> {
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/stack-headroom-watch.sw.yaml"),
    )
    .expect("watch workflow loads");
    let yaml = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/stack-headroom-watch.sw.yaml"),
    )
    .expect("watch scenario yaml reads");
    let find_task = |name: &str| {
        yaml.find(&format!("- {name}:"))
            .unwrap_or_else(|| panic!("missing task {name}"))
    };
    assert_eq!(workflow.document.name, "stack-headroom-watch");
    assert!(find_task("init_run") < find_task("preflight_state"));
    assert!(find_task("preflight_state") < find_task("await_ambient_assets"));
    assert!(find_task("await_ambient_assets") < find_task("poll_loop"));
    assert!(yaml[find_task("poll_loop")..].contains("metrics_sample"));
    assert!(yaml[find_task("poll_loop")..].contains("settle_secs: 30"));
    assert!(yaml[find_task("poll_loop")..].contains("net_liveness"));
    let mut runtime = RecordingRuntime::default();
    execute_workflow(&workflow, &mut runtime, &json!({}))?;
    let position = |name: &str| {
        runtime
            .calls
            .iter()
            .position(|call| call == name)
            .unwrap_or_else(|| panic!("missing action {name}: {:?}", runtime.calls))
    };
    assert_eq!(
        runtime
            .calls
            .iter()
            .filter(|call| call.as_str() == "metrics_sample")
            .count(),
        30
    );
    assert_eq!(
        runtime
            .calls
            .iter()
            .filter(|call| call.as_str() == "net_liveness")
            .count(),
        30
    );
    assert!(position("init_run") < position("metrics_sample"));
    assert!(position("metrics_sample") < position("net_liveness"));
    assert!(position("net_liveness") < position("cleanup_upload_off"));
    assert!(position("cleanup_upload_off") < position("finalize_report"));
    assert!(!runtime.calls.iter().any(|call| call == "fail_evidence"));
    Ok(())
}

#[test]
fn upload_off_variant_disables_upload_before_the_grid() -> Result<()> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scenarios/stack-headroom-watch-upload-off.sw.yaml");
    let workflow = load_workflow(&path).expect("upload-off workflow loads");
    let yaml = std::fs::read_to_string(&path).expect("upload-off yaml reads");
    let find_task = |name: &str| {
        yaml.find(&format!("- {name}:"))
            .unwrap_or_else(|| panic!("missing task {name}"))
    };
    assert_eq!(workflow.document.name, "stack-headroom-watch-upload-off");
    assert!(find_task("await_ambient_assets") < find_task("disable_upload"));
    assert!(find_task("disable_upload") < find_task("poll_loop"));
    assert!(yaml[find_task("disable_upload")..].contains("upload: \"off\""));
    let mut runtime = RecordingRuntime::default();
    execute_workflow(&workflow, &mut runtime, &json!({}))?;
    assert_eq!(
        runtime
            .calls
            .iter()
            .filter(|call| call.as_str() == "metrics_sample")
            .count(),
        30
    );
    assert_eq!(
        runtime
            .calls
            .iter()
            .filter(|call| call.as_str() == "net_liveness")
            .count(),
        30
    );
    let position = |name: &str| {
        runtime
            .calls
            .iter()
            .position(|call| call == name)
            .unwrap_or_else(|| panic!("missing action {name}: {:?}", runtime.calls))
    };
    assert!(position("state_command") < position("metrics_sample"));
    assert!(position("net_liveness") < position("cleanup_upload_off"));
    assert!(!runtime.calls.iter().any(|call| call == "fail_evidence"));
    Ok(())
}

#[test]
fn upload_on_variant_forces_upload_on_before_the_grid() -> Result<()> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scenarios/stack-headroom-watch-upload-on.sw.yaml");
    let workflow = load_workflow(&path).expect("upload-on workflow loads");
    let yaml = std::fs::read_to_string(&path).expect("upload-on yaml reads");
    let find_task = |name: &str| {
        yaml.find(&format!("- {name}:"))
            .unwrap_or_else(|| panic!("missing task {name}"))
    };
    assert_eq!(workflow.document.name, "stack-headroom-watch-upload-on");
    assert!(find_task("await_ambient_assets") < find_task("enable_upload"));
    assert!(find_task("enable_upload") < find_task("poll_loop"));
    assert!(yaml[find_task("enable_upload")..].contains("upload: \"on\""));
    let mut runtime = RecordingRuntime::default();
    execute_workflow(&workflow, &mut runtime, &json!({}))?;
    assert_eq!(
        runtime
            .calls
            .iter()
            .filter(|call| call.as_str() == "metrics_sample")
            .count(),
        30
    );
    let position = |name: &str| {
        runtime
            .calls
            .iter()
            .position(|call| call == name)
            .unwrap_or_else(|| panic!("missing action {name}: {:?}", runtime.calls))
    };
    assert!(position("state_command") < position("metrics_sample"));
    assert!(!runtime.calls.iter().any(|call| call == "fail_evidence"));
    Ok(())
}

#[test]
fn mountain_streaming_lifecycle_is_required_in_order() {
    for name in ["mountain_validated", "mountain_rendered"] {
        assert!(
            REQUIRED_CHECKPOINTS.contains(&name),
            "missing required checkpoint {name}"
        );
    }
    let position = |name: &str| {
        REQUIRED_CHECKPOINTS
            .iter()
            .position(|entry| *entry == name)
            .unwrap_or_else(|| panic!("missing checkpoint {name}"))
    };
    // First-generation streaming lifecycle precedes overlay adoption/UI composition.
    assert!(position("mountain_validated") < position("mountain_rendered"));
    assert!(position("mountain_rendered") < position("mountain_adopted"));
    assert!(position("mountain_adopted") < position("mountain_composed"));
    assert!(position("mountain_composed") < position("clock1_entered"));
}

#[test]
fn inferred_checkpoints_satisfy_only_proven_upstream_events() {
    assert!(checkpoint_satisfied("mountain_validated", "inferred"));
    assert!(checkpoint_satisfied("mountain_rendered", "inferred"));
    assert!(checkpoint_satisfied("clock1_assets_ready", "inferred"));
    assert!(checkpoint_satisfied("clock2_assets_ready", "inferred"));
    assert!(checkpoint_satisfied(
        "cycle_01_clock_assets_ready",
        "inferred"
    ));
    for name in [
        "cycle_01_mountain_cache_validated",
        "cycle_01_mountain_validated",
        "cycle_01_mountain_rendered",
    ] {
        assert!(checkpoint_satisfied(name, "inferred"));
    }
    assert!(!checkpoint_satisfied("mountain_adopted", "inferred"));
    assert!(!checkpoint_satisfied("clock1_frame_complete", "inferred"));
    assert!(!checkpoint_satisfied(
        "cycle_01_mountain_released",
        "inferred"
    ));
    let mut checkpoints = REQUIRED_CHECKPOINTS
        .iter()
        .map(|name| {
            (
                (*name).to_string(),
                CheckpointState {
                    status: "pass".to_string(),
                    ..Default::default()
                },
            )
        })
        .collect::<HashMap<_, _>>();
    checkpoints.get_mut("mountain_validated").unwrap().status = "inferred".to_string();
    checkpoints.get_mut("mountain_rendered").unwrap().status = "inferred".to_string();
    let required = REQUIRED_CHECKPOINTS
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    assert!(all_required_pass(&checkpoints, &required));
    checkpoints.get_mut("mountain_adopted").unwrap().status = "inferred".to_string();
    assert!(!all_required_pass(&checkpoints, &required));
}

#[test]
fn completed_clock_frame_infers_assets_but_does_not_claim_asset_timing() {
    let frame = b"CLOCK_FRAME status=complete\r\n";
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[frame]));
    runtime
        .invoke(
            "await_marker",
            &json!({
                "key": "clock_assets_ready",
                "checkpoint": "clock1_assets_ready",
                "from": "run",
                "implied_by": "clock_frame_complete"
            }),
            &mut json!({}),
        )
        .expect("complete frame proves the assets were available");
    let checkpoint = &runtime.checkpoints["clock1_assets_ready"];
    assert_eq!(checkpoint.status, "inferred");
    assert_eq!(
        checkpoint.marker_line.as_deref(),
        Some("CLOCK_FRAME status=complete")
    );
}

#[test]
fn completed_clock_frame_infers_dropped_screen_entry_marker() {
    let frame = b"CLOCK_FRAME status=complete\r\n";
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[frame]));
    runtime
        .invoke(
            "await_marker",
            &json!({
                "key": "clock_entered",
                "checkpoint": "clock1_entered",
                "from": "run",
                "implied_by": "clock_frame_complete"
            }),
            &mut json!({}),
        )
        .expect("complete frame proves the screen entered");
    assert_eq!(runtime.checkpoints["clock1_entered"].status, "inferred");
}

#[test]
fn resident_release_infers_dropped_validation_marker() {
    let released = b"MOUNTAIN_CACHE status=released reason=screen_exit bytes=665839\r\n";
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[released]));
    runtime
        .invoke(
            "await_marker",
            &json!({
                "key": "mountain_cache_validated",
                "checkpoint": "mountain_cache_validated",
                "from": "run",
                "implied_by": "mountain_released"
            }),
            &mut json!({}),
        )
        .expect("resident release proves prior validation");
    assert_eq!(
        runtime.checkpoints["mountain_cache_validated"].status,
        "inferred"
    );
}

#[test]
fn clock_asset_fallback_is_explicit_in_every_residency_scenario() {
    let scenarios = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios");
    for (name, expected_count) in [
        ("asset-residency-baseline.sw.yaml", 2),
        ("asset-residency-reentry.sw.yaml", 1),
    ] {
        let yaml = std::fs::read_to_string(scenarios.join(name)).expect("scenario source");
        assert_eq!(
            yaml.matches("key: \"clock_assets_ready\"").count(),
            expected_count,
            "{name}: number of clock asset gates changed"
        );
        assert_eq!(
            yaml.matches("implied_by: \"clock_frame_complete\"").count(),
            2 * expected_count,
            "{name}: every clock entry and asset gate needs an explicit inference label"
        );
    }
}

#[test]
fn later_adoption_explicitly_marks_missing_mountain_log_as_inferred() {
    let adopted = b"AMBIENT_MOUNTAIN status=adopted id=1 percent=5 generation=0 bytes=40350\r\n";
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[adopted]));
    let mut context = json!({});
    runtime
        .invoke(
            "await_marker",
            &json!({
                "key": "mountain_validated",
                "checkpoint": "mountain_validated",
                "from": "run",
                "implied_by": "mountain_adopted"
            }),
            &mut context,
        )
        .expect("adoption proves a successful validation path");
    let checkpoint = &runtime.checkpoints["mountain_validated"];
    assert_eq!(checkpoint.status, "inferred");
    assert_eq!(
        checkpoint.marker_line.as_deref(),
        Some(core::str::from_utf8(adopted).unwrap().trim())
    );
}

#[test]
fn observed_mountain_marker_takes_precedence_over_later_adoption() {
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[
        b"MOUNTAIN_STREAM status=validated generation=0\r\n",
        b"AMBIENT_MOUNTAIN status=adopted id=1 percent=5 generation=0 bytes=40350\r\n",
    ]));
    runtime
        .invoke(
            "await_marker",
            &json!({
                "key": "mountain_validated",
                "checkpoint": "mountain_validated",
                "from": "run",
                "implied_by": "mountain_adopted"
            }),
            &mut json!({}),
        )
        .expect("observed marker is preferred");
    assert_eq!(runtime.checkpoints["mountain_validated"].status, "pass");
}

#[test]
fn unsupported_marker_implication_is_rejected() {
    let mut runtime = silent_test_runtime(MockPort::new());
    let error = runtime
        .invoke(
            "await_marker",
            &json!({
                "key": "clock_entered",
                "checkpoint": "clock1_entered",
                "implied_by": "mountain_adopted"
            }),
            &mut json!({}),
        )
        .expect_err("unrelated marker cannot prove clock entry");
    assert!(error.to_string().contains("unsupported marker implication"));
}

#[derive(Default)]
struct RecordingRuntime {
    calls: Vec<String>,
    fail_action: Option<String>,
    failed: bool,
    null_first_probe: bool,
    null_all_probes: bool,
}

impl WorkflowRuntime for RecordingRuntime {
    fn invoke(&mut self, action: &str, _args: &Value, _context: &mut Value) -> Result<()> {
        self.calls.push(action.to_owned());
        if self.fail_action.as_deref() == Some(action) {
            self.failed = true;
            return Err(anyhow!("injected action failure: {action}"));
        }
        match action {
            "init_run"
            | "begin_cycle"
            | "begin_phase"
            | "ui_command"
            | "await_marker"
            | "record_mountain_timing"
            | "verify_mountain_ui_status"
            | "psram_snapshot"
            | "assert_no_fault"
            | "metrics_sample"
            | "net_liveness"
            | "state_command"
            | "cleanup_upload_off"
            | "print_summary"
            | "ready_pause" => Ok(()),
            "fail_evidence" => Err(anyhow!("asset residency baseline failed (injected)")),
            _ => Err(anyhow!("unsupported asset-residency action: {action}")),
        }
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        match action {
            "init_ready" => {
                self.calls.push(action.to_owned());
                Ok(Some(json!({
                    "ready": false,
                    "state_line": Value::Null,
                    "upload": Value::Null,
                    "ready_budget": vec![0u64; 4],
                    "ready_index": 0,
                })))
            }
            "state_probe" => {
                self.calls.push(action.to_owned());
                if self.fail_action.as_deref() == Some(action) {
                    self.failed = true;
                    return Err(anyhow!("injected action failure: {action}"));
                }
                if self.null_all_probes {
                    if args.get("expect_upload").is_some() {
                        self.failed = true;
                        return Err(anyhow!("STATE upload remains unknown"));
                    }
                    return Ok(Some(json!({
                        "ready": false,
                        "state_line": Value::Null,
                        "upload": Value::Null,
                    })));
                }
                if self.null_first_probe
                    && self
                        .calls
                        .iter()
                        .filter(|call| call.as_str() == "state_probe")
                        .count()
                        == 1
                {
                    return Ok(Some(json!({
                        "ready": false,
                        "state_line": Value::Null,
                        "upload": Value::Null,
                    })));
                }
                Ok(Some(json!({
                    "ready": true,
                    "state_line": "STATE phase=idle upload=off ready=true",
                    "upload": "off",
                })))
            }
            "finalize_report" => {
                self.calls.push(action.to_owned());
                Ok(Some(json!({ "run_passed": !self.failed })))
            }
            _ => {
                self.invoke(action, args, context)?;
                Ok(None)
            }
        }
    }
}

fn load_scenario() -> serverless_workflow_core::models::workflow::WorkflowDefinition {
    load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scenarios/asset-residency-baseline.sw.yaml"),
    )
    .expect("workflow loads")
}

#[test]
fn scenario_wraps_measurement_before_cleanup_and_gate() -> Result<()> {
    let workflow = load_scenario();
    let yaml = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scenarios/asset-residency-baseline.sw.yaml"),
    )
    .expect("scenario yaml reads");
    let find_task = |name: &str| {
        yaml.find(&format!("- {name}:"))
            .unwrap_or_else(|| panic!("missing task {name}"))
    };
    assert!(find_task("init_run") < find_task("preflight_state"));
    assert!(find_task("preflight_state") < find_task("normalize_upload_gate"));
    assert!(find_task("normalize_upload_off") < find_task("init_ready"));
    assert!(find_task("init_ready") < find_task("await_ambient_assets"));
    assert!(find_task("await_ambient_assets") < find_task("await_mountain_validated"));
    assert!(find_task("await_mountain_validated") < find_task("await_mountain_rendered"));
    assert!(find_task("await_mountain_rendered") < find_task("await_mountain_adopted"));
    assert!(find_task("await_mountain_adopted") < find_task("await_mountain_composed"));
    // Mountain completion precedes ready probes and snapshots: the first
    // Mountain SD job owns the console and swallows serial state commands.
    assert!(find_task("await_mountain_composed") < find_task("probe_ready"));
    assert!(find_task("probe_ready") < find_task("pause_ready"));
    assert!(find_task("await_mountain_composed") < find_task("snapshot_ready"));
    assert!(find_task("ready_gate") < find_task("snapshot_ready"));
    assert!(find_task("snapshot_ready") < find_task("snapshot_ambient"));
    assert!(find_task("preflight_state") < find_task("preflight_state_retry"));
    let gate = find_task("normalize_upload_gate");
    let default_case = yaml[gate..]
        .find("- default:")
        .map(|offset| gate + offset)
        .expect("normalize gate has a default case");
    let rest = &yaml[default_case..];
    let end = rest
        .find("\n        - ")
        .map(|offset| default_case + offset)
        .unwrap_or(yaml.len());
    let default_window = &yaml[default_case..end];
    assert!(
        default_window.contains("then: \"require_upload_off\""),
        "unknown upload state must be checked before proceeding: {default_window}"
    );
    assert!(find_task("snapshot_exit1") < find_task("step_to_launcher2"));
    assert!(find_task("await_clock1_entered") < find_task("snapshot_clock_entry"));
    assert!(find_task("snapshot_clock_entry") < find_task("await_clock1_assets"));
    assert_eq!(workflow.document.name, "asset-residency-baseline");
    let mut runtime = RecordingRuntime::default();
    execute_workflow(&workflow, &mut runtime, &json!({ "stress": false }))?;
    let position = |name: &str| {
        runtime
            .calls
            .iter()
            .position(|call| call == name)
            .unwrap_or_else(|| panic!("missing action {name}: {:?}", runtime.calls))
    };
    let first_probe = position("init_run");
    let cleanup = position("cleanup_upload_off");
    let finalize = position("finalize_report");
    let summary = position("print_summary");
    assert!(first_probe < cleanup);
    assert!(cleanup < finalize);
    assert!(finalize < summary);
    assert!(position("await_marker") > first_probe);
    assert!(position("psram_snapshot") > first_probe);
    assert!(!runtime.calls.iter().any(|call| call == "fail_evidence"));
    Ok(())
}

#[test]
fn stress_variant_enables_upload_and_probes_clock_through_rollover() -> Result<()> {
    let workflow = load_scenario();
    let yaml = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scenarios/asset-residency-baseline.sw.yaml"),
    )?;
    let find_task = |name: &str| {
        yaml.find(&format!("- {name}:"))
            .unwrap_or_else(|| panic!("missing task {name}"))
    };
    assert!(find_task("await_mountain_composed") < find_task("enable_upload_for_clock"));
    assert!(find_task("enable_upload_for_clock") < find_task("step_to_clock"));
    let mut runtime = RecordingRuntime::default();
    execute_workflow(
        &workflow,
        &mut runtime,
        &json!({ "stress": true, "stress_hold_samples": 7 }),
    )?;
    let position = |name: &str| {
        runtime
            .calls
            .iter()
            .position(|call| call == name)
            .unwrap_or_else(|| panic!("missing action {name}: {:?}", runtime.calls))
    };
    assert!(position("await_marker") < position("state_command"));
    assert_eq!(
        runtime
            .calls
            .iter()
            .filter(|call| call.as_str() == "metrics_sample")
            .count(),
        7
    );
    assert_eq!(
        runtime
            .calls
            .iter()
            .filter(|call| call.as_str() == "net_liveness")
            .count(),
        7
    );
    assert!(position("metrics_sample") < position("cleanup_upload_off"));
    assert!(!runtime.calls.iter().any(|call| call == "fail_evidence"));
    Ok(())
}

#[test]
fn association_focus_skips_clock_hold_but_keeps_cleanup() -> Result<()> {
    let workflow = load_scenario();
    let mut runtime = RecordingRuntime::default();
    execute_workflow(
        &workflow,
        &mut runtime,
        &json!({ "stress": true, "stress_hold_samples": 0 }),
    )?;
    assert!(!runtime.calls.iter().any(|call| call == "metrics_sample"));
    assert!(!runtime.calls.iter().any(|call| call == "net_liveness"));
    assert!(runtime
        .calls
        .iter()
        .any(|call| call == "cleanup_upload_off"));
    assert!(runtime.calls.iter().any(|call| call == "finalize_report"));
    assert!(!runtime.calls.iter().any(|call| call == "fail_evidence"));
    Ok(())
}

#[test]
fn default_preflight_continues_without_state_set() -> Result<()> {
    let workflow = load_scenario();
    let mut runtime = RecordingRuntime {
        null_first_probe: true,
        ..RecordingRuntime::default()
    };
    execute_workflow(&workflow, &mut runtime, &json!({ "stress": false }))?;
    let position = |name: &str| {
        runtime
            .calls
            .iter()
            .position(|call| call == name)
            .unwrap_or_else(|| panic!("missing action {name}: {:?}", runtime.calls))
    };
    assert_eq!(
        runtime.calls[..position("init_ready")]
            .iter()
            .filter(|call| call.as_str() == "state_probe")
            .count(),
        2
    );
    assert!(position("init_ready") < position("state_command"));
    assert_eq!(
        runtime
            .calls
            .iter()
            .filter(|call| call.as_str() == "state_command")
            .count(),
        1
    );
    assert!(!runtime.calls.iter().any(|call| call == "fail_evidence"));
    Ok(())
}

#[test]
fn unreadable_preflight_stops_before_asset_markers() -> Result<()> {
    let workflow = load_scenario();
    let mut runtime = RecordingRuntime {
        null_all_probes: true,
        ..RecordingRuntime::default()
    };
    execute_workflow(&workflow, &mut runtime, &json!({ "stress": false }))
        .expect_err("unknown upload state must stop the run");
    assert_eq!(
        runtime
            .calls
            .iter()
            .filter(|call| call.as_str() == "state_probe")
            .count(),
        3
    );
    assert!(!runtime.calls.iter().any(|call| call == "await_marker"));
    assert!(!runtime.calls.iter().any(|call| call == "state_command"));
    Ok(())
}

#[test]
fn injected_measurement_failure_still_runs_cleanup_and_finalize() {
    let workflow = load_scenario();
    let mut runtime = RecordingRuntime {
        fail_action: Some("ui_command".to_owned()),
        ..RecordingRuntime::default()
    };
    let error = execute_workflow(&workflow, &mut runtime, &json!({ "stress": false }))
        .expect_err("injected failure must fail the run");
    assert!(error
        .to_string()
        .contains("asset residency baseline failed"));
    let failed_at = runtime
        .calls
        .iter()
        .position(|call| call == "ui_command")
        .expect("failing action ran");
    let cleanup = runtime
        .calls
        .iter()
        .position(|call| call == "cleanup_upload_off")
        .expect("cleanup ran");
    let finalize = runtime
        .calls
        .iter()
        .position(|call| call == "finalize_report")
        .expect("finalize ran");
    let fail = runtime
        .calls
        .iter()
        .position(|call| call == "fail_evidence")
        .expect("gate failed the run");
    assert!(failed_at < cleanup);
    assert!(cleanup < finalize);
    assert!(finalize < fail);
}

fn silent_test_runtime(port: MockPort) -> AssetResidencyRuntime<'static> {
    // Leak the logger so the runtime can hold `&mut` without fighting the
    // test frame's lifetime. Test-only.
    let logger: &'static mut Logger = Box::leak(Box::new(Logger::new(None).expect("logger")));
    let console = SerialConsole::from_port_for_tests(Box::new(port), None).expect("console");
    AssetResidencyRuntime {
        logger,
        console,
        consecutive_misses: 0,
        last_net: None,
        feature_label: "test".to_string(),
        build_label: "test".to_string(),
        marker_timeout: Duration::from_secs(1),
        started_at: "2026-09-26T00:00:00Z".to_string(),
        run_start: Instant::now(),
        run_mark: 0,
        phase_name: "test".to_string(),
        phase_mark: 0,
        report_path: PathBuf::from("test-report.json"),
        required_checkpoints: REQUIRED_CHECKPOINTS
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        active_cycle: None,
        checkpoints: HashMap::new(),
        state_lines: Vec::new(),
        report: None,
    }
}

#[test]
fn unanswered_sample_streak_aborts_the_grid() {
    let mut runtime = silent_test_runtime(MockPort::new());
    let mut context = json!({});
    let args = json!({"settle_secs": 0, "abort_after_consecutive_misses": 1});
    let err = runtime
        .invoke("metrics_sample", &args, &mut context)
        .expect_err("first miss aborts at limit 1");
    assert!(
        err.to_string().contains("consecutive"),
        "abort names the hang: {err}"
    );
    assert_eq!(
        runtime.state_lines.len(),
        1,
        "miss still records a trend line"
    );
    assert!(runtime.state_lines[0].contains("main_minimum=missing"));
}

#[test]
fn answered_sample_resets_the_miss_streak() {
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[
        b"METRICS NET_ACCEPT arm_gap_n=3\r\n",
        b"stack_diag: tag=minimum headroom=80000\r\n",
        b"touch_core_stack_diag: tag=minimum headroom=3300\r\n",
        b"STACK_STATUS cpu0=80132 touch=3300 tx_drop=7\r\n",
    ]));
    let mut context = json!({});
    let args = json!({"settle_secs": 0, "abort_after_consecutive_misses": 2});
    runtime
        .invoke("metrics_sample", &args, &mut context)
        .expect("answered sample continues");
    assert_eq!(runtime.consecutive_misses, 0);
    assert!(runtime.state_lines[0].contains("main_minimum=80132"));
    assert!(runtime.state_lines[0].contains("tx_drop=7"));
    assert!(runtime.state_lines[0].contains("stack_answered=1"));
    // The scripted reply is spent; the next sample misses but must not abort.
    runtime
        .invoke("metrics_sample", &args, &mut context)
        .expect("first miss after an answer continues");
    assert_eq!(runtime.consecutive_misses, 1);
}

#[test]
fn correlated_stack_status_overrides_missing_diagnostic_lines() {
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[
        b"METRICS NET_ACCEPT arm_gap_n=3\r\n",
        b"STACK_STATUS cpu0=80132 touch=3300 tx_drop=2\r\n",
    ]));
    runtime
        .invoke(
            "metrics_sample",
            &json!({"settle_secs": 0, "abort_after_consecutive_misses": 1}),
            &mut json!({}),
        )
        .expect("correlated status proves serial liveness and minima");
    assert!(runtime.state_lines[0].contains("main_minimum=80132"));
    assert!(runtime.state_lines[0].contains("touch_minimum=3300"));
    assert!(runtime.state_lines[0].contains("tx_drop=2"));
    assert!(runtime.state_lines[0].contains("metrics_answered=1 stack_answered=1"));
}

#[test]
fn stack_status_parser_requires_exact_correlated_fields() {
    assert_eq!(
        stack_status_values("STACK_STATUS cpu0=80132 touch=3300 tx_drop=2"),
        Some((80132, 3300, 2))
    );
    assert_eq!(
        stack_status_values("stack_diag: tag=minimum headroom=80132"),
        None
    );
    assert_eq!(
        stack_status_values("STACK_STATUS cpu0=80132 touch=3300"),
        None
    );
    assert_eq!(
        stack_status_values("STACK_STATUS cpu0=80132 touch=3300 tx_drop=2 extra=1"),
        None
    );
}

#[test]
fn listener_snoop_parses_socket_address() {
    assert_eq!(
        snooped_listener("upload_http: listening on 192.168.114.40:8080"),
        Some("192.168.114.40:8080".parse().expect("addr"))
    );
    assert_eq!(snooped_listener("tap_trace,164131,0x00"), None);
    assert_eq!(snooped_listener(""), None);
    assert_eq!(snooped_listener("listening on"), None);
    assert_eq!(snooped_listener("listening on not-an-address"), None);
}

#[test]
fn metrics_listener_requires_live_listener_and_routable_ip() {
    assert_eq!(
        metrics_listener(
            "METRICS NET wifi_connected=1 http_listening=1 ip=192.168.114.40",
            8080,
            false
        ),
        Some("192.168.114.40:8080".parse().expect("addr"))
    );
    assert_eq!(
        metrics_listener(
            "METRICS NET wifi_connected=0 http_listening=1 ip=192.168.114.40",
            8080,
            false
        ),
        None
    );
    assert_eq!(
        metrics_listener(
            "METRICS NET wifi_connected=1 http_listening=0 ip=192.168.114.40",
            8080,
            false
        ),
        None
    );
    assert_eq!(
        metrics_listener(
            "METRICS NET wifi_connected=1 http_listening=1 ip=0.0.0.0",
            8080,
            false
        ),
        None
    );
    assert_eq!(
        metrics_listener("METRICS NET_ACCEPT arm_gap_n=0", 8080, false),
        None
    );
    assert_eq!(
        metrics_listener(
            "METRICS NET wifi_connected=1 http_listening=0 ip=192.168.114.40",
            8080,
            true
        ),
        Some("192.168.114.40:8080".parse().expect("addr"))
    );
    assert_eq!(
        metrics_listener(
            "METRICS NET wifi_connected=0 http_listening=0 ip=192.168.114.40",
            8080,
            true
        ),
        None
    );
}

#[test]
fn net_liveness_uses_metrics_when_listener_announcement_was_lost() {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe listener");
    let port = probe.local_addr().expect("port").port();
    let feed = b"METRICS NET wifi_connected=1 http_listening=1 ip=127.0.0.1\r\n";
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[feed]));
    let mut context = json!({});
    runtime
        .invoke(
            "net_liveness",
            &json!({"timeout_secs": 2, "port": port}),
            &mut context,
        )
        .expect("metrics fallback probe runs");
    assert!(
        runtime.state_lines[0].contains("reachable=true"),
        "fallback line: {}",
        runtime.state_lines[0]
    );
}

#[test]
fn net_liveness_reports_reachable_listener() {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe listener");
    let port = probe.local_addr().expect("port").port();
    let feed = format!("upload_http: listening on 127.0.0.1:{port}\r\n");
    let chunk: &[u8] = feed.as_bytes();
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[chunk]));
    let mut context = json!({});
    runtime
        .invoke("net_liveness", &json!({"timeout_secs": 2}), &mut context)
        .expect("probe runs");
    assert!(
        runtime.state_lines[0].contains("reachable=true"),
        "up line: {}",
        runtime.state_lines[0]
    );
    assert!(
        runtime
            .last_net
            .as_deref()
            .unwrap_or_default()
            .starts_with("up "),
        "summary names up: {:?}",
        runtime.last_net
    );
}

#[test]
fn net_liveness_reports_refused_port() {
    // Bind then drop so the port reads back as closed.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("port")
        .port();
    let feed = format!("upload_http: listening on 127.0.0.1:{port}\r\n");
    let chunk: &[u8] = feed.as_bytes();
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[chunk]));
    let mut context = json!({});
    runtime
        .invoke("net_liveness", &json!({"timeout_secs": 2}), &mut context)
        .expect("refused probe still records");
    assert!(
        runtime.state_lines[0].contains("reachable=false"),
        "down line: {}",
        runtime.state_lines[0]
    );
    assert!(runtime.state_lines[0].contains("responded=1"));
}

#[test]
fn net_liveness_accepts_refused_response_with_upload_off() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("port")
        .port();
    let feed = b"METRICS NET wifi_connected=1 http_listening=0 ip=127.0.0.1\r\n";
    let mut runtime = silent_test_runtime(MockPort::new().pushed_reads(&[feed]));
    let mut context = json!({});
    runtime
        .invoke(
            "net_liveness",
            &json!({"timeout_secs": 2, "port": port, "allow_closed_listener": true, "require_response": true}),
            &mut context,
        )
        .expect("closed listener returned a TCP refusal");
    assert!(runtime.state_lines[0].contains("reachable=false"));
    assert!(runtime.state_lines[0].contains("responded=1"));
}

#[test]
fn net_liveness_without_listener_is_unknown() {
    let mut runtime = silent_test_runtime(MockPort::new());
    let mut context = json!({});
    runtime
        .invoke("net_liveness", &json!({}), &mut context)
        .expect("missing listener is not an error");
    assert!(
        runtime.state_lines[0].contains("reachable=unknown"),
        "unknown line: {}",
        runtime.state_lines[0]
    );
    assert!(runtime.state_lines[0].contains("responded=0"));
}

#[test]
fn net_liveness_requires_response_when_requested() {
    let mut runtime = silent_test_runtime(MockPort::new());
    let mut context = json!({});
    let error = runtime
        .invoke(
            "net_liveness",
            &json!({"require_response": true}),
            &mut context,
        )
        .expect_err("unknown target is not a TCP response");
    assert!(error.to_string().contains("did not get a TCP response"));
}
