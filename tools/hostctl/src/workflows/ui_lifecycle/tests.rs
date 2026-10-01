use std::path::PathBuf;

use super::analysis::analyze_lines;
use crate::scenarios::load_workflow;

fn lifecycle(surface: u16, used: usize, phase: &str) -> String {
    format!(
        "LVGL_LIFECYCLE phase={phase} active=Some(SurfaceInstanceToken {{ surface: SurfaceRef {{ owner: ProviderToken {{ id: ProviderId(1), generation: ProviderGeneration(1) }}, id: SurfaceId({surface}) }}, generation: InstanceGeneration(1) }}) shell_aligned=true transition_us=10 lvgl_total=128000 lvgl_used={used} lvgl_used_blocks={} lvgl_max_used=200 lvgl_frag_pct=2 integrity_ok=true heap_internal_free=20000 heap_internal_min=20000 heap_external_free=100000 heap_external_min=100000 heap_peak_used=50000 cpu0_stack_min=80000 timer_gap_max_us=9000 timer_runtime_max_us=100 cleanup_blocked=false navigation_faulted=false",
        100 + surface
    )
}

fn passing_lines() -> Vec<String> {
    let mut lines = Vec::new();
    for (surface, name, used) in [
        (2, "launcher", 120usize),
        (3, "diagnostics", 160),
        (1, "home", 100),
        (2, "launcher", 120),
        (3, "diagnostics", 160),
        (1, "home", 100),
    ] {
        lines.push(lifecycle(surface, used + 40, "candidate_created"));
        lines.push(lifecycle(surface, used, "settled_after_delete"));
        lines.push(format!("UI_CYCLE_VISIBLE surface={name} status=ok"));
    }
    lines
}

#[test]
fn accepts_two_complete_cycles_with_restored_surface_baselines() {
    let report = analyze_lines(&passing_lines(), 2, 0);
    assert!(report.run_passed, "{:?}", report.violations);
}

#[test]
fn rejects_settled_baseline_drift() {
    let mut lines = passing_lines();
    let index = lines
        .iter()
        .rposition(|line| line.contains("SurfaceId(1)") && line.contains("settled_after_delete"))
        .expect("home sample");
    lines[index] = lifecycle(1, 108, "settled_after_delete");
    let report = analyze_lines(&lines, 2, 0);
    assert!(!report.run_passed);
    assert!(report
        .violations
        .iter()
        .any(|violation| violation.contains("baseline span")));
}

#[test]
fn accepts_an_explicit_bounded_allocator_settling_band() {
    let mut lines = passing_lines();
    let index = lines
        .iter()
        .rposition(|line| line.contains("SurfaceId(1)") && line.contains("settled_after_delete"))
        .expect("home sample");
    lines[index] =
        lifecycle(1, 228, "settled_after_delete").replace("lvgl_max_used=200", "lvgl_max_used=228");
    let report = analyze_lines(&lines, 2, 256);
    assert!(report.run_passed, "{:?}", report.violations);
}

#[test]
fn rejects_health_failures_and_incomplete_routes() {
    let mut lines = passing_lines();
    lines.pop();
    lines.push("LVGL_LIFECYCLE phase=settled_after_delete shell_aligned=false".to_string());
    let report = analyze_lines(&lines, 2, 0);
    assert!(!report.run_passed);
    assert!(report
        .violations
        .iter()
        .any(|violation| violation.contains("shell_aligned=false")));
}

#[test]
fn rejects_allocator_and_stack_minima_below_runtime_floors() {
    let mut lines = passing_lines();
    let index = lines
        .iter()
        .position(|line| line.contains("settled_after_delete"))
        .expect("settled sample");
    lines[index] = lines[index]
        .replace("heap_internal_min=20000", "heap_internal_min=16383")
        .replace("cpu0_stack_min=80000", "cpu0_stack_min=8191");
    let report = analyze_lines(&lines, 2, 0);
    assert!(!report.run_passed);
    assert!(report.violations.iter().any(|v| v.contains("CPU0 stack")));
    assert!(report
        .violations
        .iter()
        .any(|v| v.contains("internal heap")));
}

#[test]
fn rejects_inconsistent_lvgl_usage_and_peak_fields() {
    let mut lines = passing_lines();
    let index = lines
        .iter()
        .position(|line| line.contains("settled_after_delete"))
        .expect("settled sample");
    lines[index] = lines[index]
        .replace("lvgl_total=128000", "lvgl_total=100")
        .replace("lvgl_max_used=200", "lvgl_max_used=99");
    let report = analyze_lines(&lines, 2, 0);
    assert!(!report.run_passed);
    assert!(report
        .violations
        .iter()
        .any(|v| v.contains("exceeds total")));
    assert!(report.violations.iter().any(|v| v.contains("peak")));
}

#[test]
fn workflow_keeps_repetition_and_evidence_gate_in_yaml() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ui-lifecycle.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    assert_eq!(workflow.document.name, "ui-lifecycle");
    let raw = std::fs::read_to_string(path).expect("workflow source");
    assert!(raw.contains("repeat:"));
    assert!(raw.contains("call: \"run_repaint\""));
    assert!(raw.contains("fail_evidence"));
    assert!(raw.contains("call: \"print_summary\"\n      then: \"__end__\""));
}

#[test]
fn rejects_composition_and_callback_lifetime_faults() {
    for marker in ["composition_faulted=true", "lifecycle_audit_faulted=true"] {
        let mut lines = passing_lines();
        lines.push(format!(
            "LVGL_LIFECYCLE phase=settled_after_delete {marker}"
        ));
        let report = analyze_lines(&lines, 2, 0);
        assert!(!report.run_passed);
        assert!(report.violations.iter().any(|v| v.contains(marker)));
    }
}

#[test]
fn workflow_queries_live_state_and_requires_correlated_repaint_completion() -> anyhow::Result<()> {
    use super::{execute_workflow, json, Logger, SerialConsole, UiLifecycleRuntime};
    use serialport::{SerialPort, TTYPort};
    use std::{
        io::{Read, Write},
        thread,
        time::Duration,
    };

    for status in ["Completed", "Failed"] {
        let (mut master, mut slave) = TTYPort::pair()?;
        slave.set_timeout(Duration::from_millis(5))?;
        master.set_timeout(Duration::from_secs(1))?;
        let responder = thread::spawn(move || -> anyhow::Result<Vec<String>> {
            let mut commands = Vec::new();
            for index in 0..if status == "Completed" { 8 } else { 2 } {
                let mut bytes = Vec::new();
                loop {
                    let mut byte = [0];
                    master.read_exact(&mut byte)?;
                    if byte[0] == b'\n' {
                        break;
                    }
                    bytes.push(byte[0]);
                }
                let command = String::from_utf8(bytes)?.trim().to_owned();
                match index {
                    0 => {
                        assert_eq!(command, "STATE GET");
                        writeln!(master, "STATE phase=OPERATING upload=off diag_kind=NONE targets=NONE ready=true\r")?;
                    }
                    1 => {
                        let id: u64 = command.strip_prefix("REPAINT ").unwrap().parse()?;
                        writeln!(
                            master,
                            "PANEL_FIXTURE END id={} status=Completed at_ms=1\r",
                            id + 1
                        )?;
                        writeln!(
                            master,
                            "PANEL_FIXTURE END id={id} status={status} at_ms=2\r"
                        )?;
                    }
                    _ => {
                        assert_eq!(command, "UISTEP");
                        let start = (index - 2) * 3;
                        for line in &passing_lines()[start..start + 3] {
                            writeln!(master, "{line}\r")?;
                        }
                        writeln!(master, "UISTEP OK\r")?;
                    }
                }
                master.flush()?;
                commands.push(command);
            }
            // Keep the PTY alive until its final reply has been consumed.
            thread::sleep(Duration::from_millis(100));
            Ok(commands)
        });
        let directory = tempfile::tempdir()?;
        let log_path = directory.path().join("ui.log");
        let console = SerialConsole::from_port_for_tests(Box::new(slave), Some(&log_path))?;
        let mut logger = Logger::new(None)?;
        let mut runtime = UiLifecycleRuntime {
            logger: &mut logger,
            console,
            cycles: 2,
            max_baseline_drift_bytes: 0,
            evidence_mark: 0,
            log_path,
            report: None,
            repaint_request_id: None,
        };
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ui-lifecycle.sw.yaml"),
        )?;
        let result = execute_workflow(&workflow, &mut runtime, &json!({}));
        let commands = responder.join().unwrap()?;
        if status == "Completed" {
            result?;
            let report = runtime.report.unwrap();
            assert!(report.run_passed);
            assert!(report.repaint_request_id.is_some());
            assert_eq!(commands.len(), 8);
        } else {
            let error = format!("{:#}", result.unwrap_err());
            assert!(error.contains("repaint failed"), "{error}");
            assert!(runtime.repaint_request_id.is_none());
            assert_eq!(commands.len(), 2);
        }
    }
    Ok(())
}
