use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, Result};
use chrono::Local;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::{AckStatus, SerialConsole},
};

const STEP_TIMEOUT: Duration = Duration::from_secs(180);
const READY_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub struct UiLifecycleOptions {
    pub cycles: u16,
    pub max_baseline_drift_bytes: usize,
    pub output_path: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct UiLifecycleReport {
    cycles_requested: u16,
    max_baseline_drift_bytes: usize,
    steps_expected: usize,
    visible_surfaces: Vec<String>,
    candidate_checkpoints: usize,
    settled_checkpoints: usize,
    settled_samples_by_surface: BTreeMap<u16, Vec<usize>>,
    settled_total_by_surface: BTreeMap<u16, Vec<usize>>,
    settled_used_blocks_by_surface: BTreeMap<u16, Vec<usize>>,
    settled_frag_pct_by_surface: BTreeMap<u16, Vec<usize>>,
    high_water_plateau_window_steps: usize,
    high_water_plateau: bool,
    current_heap_stable: bool,
    max_transition_us: usize,
    max_candidate_lvgl_used: usize,
    min_cpu0_stack_headroom: Option<usize>,
    min_internal_heap_free: Option<usize>,
    min_external_heap_free: Option<usize>,
    max_timer_gap_us: usize,
    max_timer_runtime_us: usize,
    violations: Vec<String>,
    run_passed: bool,
}

fn parse_usize_key(line: &str, key: &str) -> Option<usize> {
    line.split_ascii_whitespace()
        .find_map(|field| field.strip_prefix(key))?
        .trim_end_matches(|character: char| !character.is_ascii_digit())
        .parse()
        .ok()
}

fn update_min(slot: &mut Option<usize>, value: usize) {
    *slot = Some(slot.map_or(value, |current| current.min(value)));
}

fn sample_span(samples: &[usize]) -> usize {
    match (samples.iter().min(), samples.iter().max()) {
        (Some(minimum), Some(maximum)) => maximum - minimum,
        _ => 0,
    }
}

fn all_equal(samples: &[usize]) -> bool {
    samples
        .first()
        .is_none_or(|first| samples.iter().all(|sample| sample == first))
}

fn analyze_lines(
    lines: &[String],
    cycles: u16,
    max_baseline_drift_bytes: usize,
) -> UiLifecycleReport {
    let steps_expected = usize::from(cycles) * 3;
    let visible_re = Regex::new(r"UI_CYCLE_VISIBLE surface=(home|launcher|diagnostics) status=ok")
        .expect("static regex");
    let settled_re = Regex::new(
        r"LVGL_LIFECYCLE phase=settled_after_delete active=Some\(SurfaceInstanceToken \{ surface: SurfaceRef \{.*?id: SurfaceId\(([0-9]+)\)",
    )
    .expect("static regex");
    let mut visible_surfaces = Vec::new();
    let mut settled_samples_by_surface = BTreeMap::<u16, Vec<usize>>::new();
    let mut settled_total_by_surface = BTreeMap::<u16, Vec<usize>>::new();
    let mut settled_used_blocks_by_surface = BTreeMap::<u16, Vec<usize>>::new();
    let mut settled_frag_pct_by_surface = BTreeMap::<u16, Vec<usize>>::new();
    let mut settled_high_water = Vec::new();
    let mut settled_internal_heap_free = Vec::new();
    let mut settled_external_heap_free = Vec::new();
    let mut candidate_checkpoints = 0usize;
    let mut settled_checkpoints = 0usize;
    let mut violations = Vec::new();
    let mut max_transition_us = 0usize;
    let mut max_candidate_lvgl_used = 0usize;
    let mut min_cpu0_stack_headroom = None;
    let mut min_internal_heap_free = None;
    let mut min_external_heap_free = None;
    let mut max_timer_gap_us = 0usize;
    let mut max_timer_runtime_us = 0usize;

    for (index, line) in lines.iter().enumerate() {
        if let Some(capture) = visible_re.captures(line) {
            visible_surfaces.push(capture[1].to_string());
        }
        if line.contains("LVGL_LIFECYCLE phase=candidate_created") {
            candidate_checkpoints += 1;
            match (
                parse_usize_key(line, "transition_us="),
                parse_usize_key(line, "lvgl_used="),
                parse_usize_key(line, "cpu0_stack_min="),
            ) {
                (Some(transition), Some(used), Some(stack)) => {
                    max_transition_us = max_transition_us.max(transition);
                    max_candidate_lvgl_used = max_candidate_lvgl_used.max(used);
                    update_min(&mut min_cpu0_stack_headroom, stack);
                }
                _ => violations.push(format!(
                    "line {} has a malformed candidate lifecycle checkpoint",
                    index + 1
                )),
            }
        }
        if line.contains("LVGL_LIFECYCLE phase=settled_after_delete") {
            settled_checkpoints += 1;
            match settled_re.captures(line) {
                Some(capture) => {
                    let surface = capture[1].parse::<u16>();
                    let fields = (
                        parse_usize_key(line, "lvgl_used="),
                        parse_usize_key(line, "lvgl_total="),
                        parse_usize_key(line, "lvgl_used_blocks="),
                        parse_usize_key(line, "lvgl_frag_pct="),
                        parse_usize_key(line, "lvgl_max_used="),
                    );
                    match (surface, fields) {
                        (
                            Ok(surface),
                            (
                                Some(used),
                                Some(total),
                                Some(used_blocks),
                                Some(frag),
                                Some(high_water),
                            ),
                        ) => {
                            settled_samples_by_surface
                                .entry(surface)
                                .or_default()
                                .push(used);
                            settled_total_by_surface
                                .entry(surface)
                                .or_default()
                                .push(total);
                            settled_used_blocks_by_surface
                                .entry(surface)
                                .or_default()
                                .push(used_blocks);
                            settled_frag_pct_by_surface
                                .entry(surface)
                                .or_default()
                                .push(frag);
                            settled_high_water.push(high_water);
                        }
                        _ => violations.push(format!(
                            "line {} has invalid settled lifecycle fields",
                            index + 1
                        )),
                    }
                }
                None => violations.push(format!(
                    "line {} has a malformed settled lifecycle checkpoint",
                    index + 1
                )),
            }
            for (key, maximum) in [
                ("transition_us=", &mut max_transition_us),
                ("timer_gap_max_us=", &mut max_timer_gap_us),
                ("timer_runtime_max_us=", &mut max_timer_runtime_us),
            ] {
                match parse_usize_key(line, key) {
                    Some(value) => *maximum = (*maximum).max(value),
                    None => violations.push(format!(
                        "line {} is missing lifecycle field `{}`",
                        index + 1,
                        key
                    )),
                }
            }
            for (key, minimum) in [
                ("cpu0_stack_min=", &mut min_cpu0_stack_headroom),
                ("heap_internal_free=", &mut min_internal_heap_free),
                ("heap_external_free=", &mut min_external_heap_free),
            ] {
                match parse_usize_key(line, key) {
                    Some(value) => update_min(minimum, value),
                    None => violations.push(format!(
                        "line {} is missing lifecycle field `{}`",
                        index + 1,
                        key
                    )),
                }
            }
            if let Some(value) = parse_usize_key(line, "heap_internal_free=") {
                settled_internal_heap_free.push(value);
            }
            if let Some(value) = parse_usize_key(line, "heap_external_free=") {
                settled_external_heap_free.push(value);
            }
        }

        for marker in [
            "shell_aligned=false",
            "integrity_ok=false",
            "cleanup_blocked=true",
            "navigation_faulted=true",
            "UI_NAV state=rolled_back",
            "UI_NAV state=fault",
            "UI_NAV state=rejected",
            "LVGL_REFRESH phase=ui_cycle_step source=serial_ui_step status=error",
            "Guru Meditation",
            "watchdog",
            "panicked at",
            "rst:",
        ] {
            if line.contains(marker) {
                violations.push(format!("line {} contains `{}`", index + 1, marker));
            }
        }
        if line.contains("LVGL_REFRESH phase=ui_cycle_step source=serial_ui_step")
            && (line.contains("kind=full") || line.contains("full_fallback=true"))
        {
            violations.push(format!(
                "line {} used an unexpected full refresh during UI cycling",
                index + 1
            ));
        }
    }

    let expected_cycle = ["launcher", "diagnostics", "home"];
    let expected_surfaces = (0..cycles)
        .flat_map(|_| expected_cycle)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if visible_surfaces != expected_surfaces {
        violations.push(format!(
            "visible route mismatch: expected {:?}, observed {:?}",
            expected_surfaces, visible_surfaces
        ));
    }
    if candidate_checkpoints != steps_expected {
        violations.push(format!(
            "candidate checkpoint count {} != {}",
            candidate_checkpoints, steps_expected
        ));
    }
    if settled_checkpoints != steps_expected {
        violations.push(format!(
            "settled checkpoint count {} != {}",
            settled_checkpoints, steps_expected
        ));
    }
    for (surface, expected_count) in [(1u16, cycles), (2, cycles), (3, cycles)] {
        let samples = settled_samples_by_surface
            .get(&surface)
            .cloned()
            .unwrap_or_default();
        if samples.len() != usize::from(expected_count) {
            violations.push(format!(
                "surface {} settled samples {} != {}",
                surface,
                samples.len(),
                expected_count
            ));
        }
        let used_span = sample_span(&samples);
        if used_span > max_baseline_drift_bytes {
            violations.push(format!(
                "surface {} LVGL used baseline span {} exceeds {} bytes: {:?}",
                surface, used_span, max_baseline_drift_bytes, samples
            ));
        }

        let total_samples = settled_total_by_surface
            .get(&surface)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let total_span = sample_span(total_samples);
        if total_span > max_baseline_drift_bytes {
            violations.push(format!(
                "surface {} LVGL usable-total span {} exceeds {} bytes: {:?}",
                surface, total_span, max_baseline_drift_bytes, total_samples
            ));
        }

        let used_blocks = settled_used_blocks_by_surface
            .get(&surface)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if !all_equal(used_blocks) {
            violations.push(format!(
                "surface {} LVGL used-block count changed: {:?}",
                surface, used_blocks
            ));
        }

        let frag_pct = settled_frag_pct_by_surface
            .get(&surface)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if sample_span(frag_pct) > 1 {
            violations.push(format!(
                "surface {} LVGL fragmentation varied by more than one point: {:?}",
                surface, frag_pct
            ));
        }
    }

    let high_water_plateau_window_steps = if cycles >= 10 {
        settled_high_water.len().min(30)
    } else {
        0
    };
    let high_water_plateau = high_water_plateau_window_steps == 0
        || all_equal(
            &settled_high_water[settled_high_water.len() - high_water_plateau_window_steps..],
        );
    if !high_water_plateau {
        violations.push(format!(
            "LVGL high-water did not plateau over the final {} transitions",
            high_water_plateau_window_steps
        ));
    }

    let current_heap_stable =
        all_equal(&settled_internal_heap_free) && all_equal(&settled_external_heap_free);
    if !current_heap_stable {
        violations
            .push("current internal or external heap free drifted during the run".to_string());
    }

    UiLifecycleReport {
        cycles_requested: cycles,
        max_baseline_drift_bytes,
        steps_expected,
        visible_surfaces,
        candidate_checkpoints,
        settled_checkpoints,
        settled_samples_by_surface,
        settled_total_by_surface,
        settled_used_blocks_by_surface,
        settled_frag_pct_by_surface,
        high_water_plateau_window_steps,
        high_water_plateau,
        current_heap_stable,
        max_transition_us,
        max_candidate_lvgl_used,
        min_cpu0_stack_headroom,
        min_internal_heap_free,
        min_external_heap_free,
        max_timer_gap_us,
        max_timer_runtime_us,
        run_passed: violations.is_empty(),
        violations,
    }
}

fn report_path_for(log_path: &Path) -> PathBuf {
    let mut path = log_path.to_path_buf();
    path.set_extension("json");
    path
}

struct UiLifecycleRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    cycles: u16,
    max_baseline_drift_bytes: usize,
    evidence_mark: usize,
    log_path: PathBuf,
    report: Option<UiLifecycleReport>,
}

impl WorkflowRuntime for UiLifecycleRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, _context: &mut Value) -> Result<()> {
        match action {
            "await_ready" => {
                let ready = Regex::new(r"RUNTIME_READY app_state=ready display=ready")?;
                self.console
                    .wait_for_regex_since(0, &ready, READY_TIMEOUT)?
                    .ok_or_else(|| anyhow!("device did not report runtime ready"))?;
                Ok(())
            }
            "run_step" => {
                let mark = self.console.mark();
                self.console.send_line("UISTEP")?;
                let (status, line) = self.console.wait_ack_since(mark, "UISTEP", STEP_TIMEOUT)?;
                match status {
                    AckStatus::Ok => Ok(()),
                    AckStatus::None => Err(anyhow!(
                        "UISTEP timed out; outcome is ambiguous and the run stopped without retry"
                    )),
                    AckStatus::Busy | AckStatus::Err => Err(anyhow!(
                        "UISTEP failed: {}",
                        line.unwrap_or_else(|| "missing response".to_string())
                    )),
                }
            }
            "print_summary" => {
                let report = self
                    .report
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing report"))?;
                self.logger.info(format!(
                    "UI lifecycle passed: cycles={} steps={} report={}",
                    report.cycles_requested,
                    report.steps_expected,
                    report_path_for(&self.log_path).display()
                ));
                Ok(())
            }
            "fail_evidence" => {
                let report = self
                    .report
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing report"))?;
                Err(anyhow!(
                    "UI lifecycle evidence failed: {}",
                    report.violations.join("; ")
                ))
            }
            other => Err(anyhow!("unsupported ui-lifecycle action: {other}")),
        }
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        _args: &Value,
        _context: &mut Value,
    ) -> Result<Option<Value>> {
        match action {
            "init_run" => {
                self.evidence_mark = self.console.mark();
                Ok(Some(json!({
                    "step_count": usize::from(self.cycles) * 3,
                    "step_index": 0
                })))
            }
            "analyze_evidence" => {
                let lines = self.console.read_recent_lines(self.evidence_mark);
                let report = analyze_lines(&lines, self.cycles, self.max_baseline_drift_bytes);
                let run_passed = report.run_passed;
                let path = report_path_for(&self.log_path);
                ensure_parent_dir(&path)?;
                std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
                self.report = Some(report);
                Ok(Some(json!({ "run_passed": run_passed })))
            }
            _ => {
                self.invoke(action, _args, _context)?;
                Ok(None)
            }
        }
    }
}

pub fn run_ui_lifecycle(logger: &mut Logger, opts: UiLifecycleOptions) -> Result<()> {
    if !(2..=100).contains(&opts.cycles) {
        return Err(anyhow!("cycles must be in 2..=100"));
    }
    let log_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(format!(
            "logs/ui_lifecycle_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ))
    });
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115_200)?;
    ensure_parent_dir(&log_path)?;
    let console = SerialConsole::open(&port, baud, Some(&log_path))?;
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ui-lifecycle.sw.yaml"),
    )?;
    let mut runtime = UiLifecycleRuntime {
        logger,
        console,
        cycles: opts.cycles,
        max_baseline_drift_bytes: opts.max_baseline_drift_bytes,
        evidence_mark: 0,
        log_path,
        report: None,
    };
    let _ = execute_workflow(&workflow, &mut runtime, &json!({ "cycles": opts.cycles }))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::analyze_lines;
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
            .rposition(|line| {
                line.contains("SurfaceId(1)") && line.contains("settled_after_delete")
            })
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
            .rposition(|line| {
                line.contains("SurfaceId(1)") && line.contains("settled_after_delete")
            })
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
}
