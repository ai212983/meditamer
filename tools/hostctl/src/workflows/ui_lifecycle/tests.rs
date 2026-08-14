use std::path::PathBuf;

use super::analysis::analyze_lines;
use crate::scenarios::load_workflow;

fn lifecycle(surface: u16, used: usize, phase: &str) -> String {
    format!(
        "LVGL_LIFECYCLE phase={phase} active=Some(SurfaceInstanceToken {{ surface: SurfaceRef {{ owner: ProviderToken {{ id: ProviderId(1), generation: ProviderGeneration(1) }}, id: SurfaceId({surface}) }}, generation: InstanceGeneration(1) }}) shell_aligned=true transition_us=10 lvgl_total=128000 lvgl_used={used} lvgl_used_blocks={} lvgl_max_used=200 lvgl_frag_pct=2 integrity_ok=true heap_internal_free=20000 heap_external_free=100000 cpu0_stack_min=80000 timer_gap_max_us=9000 timer_runtime_max_us=100 cleanup_blocked=false navigation_faulted=false",
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
    lines[index] = lifecycle(1, 228, "settled_after_delete");
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
fn workflow_keeps_repetition_and_evidence_gate_in_yaml() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ui-lifecycle.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    assert_eq!(workflow.document.name, "ui-lifecycle");
    let raw = std::fs::read_to_string(path).expect("workflow source");
    assert!(raw.contains("repeat:"));
    assert!(raw.contains("fail_evidence"));
    assert!(raw.contains("call: \"print_summary\"\n      then: \"__end__\""));
}
