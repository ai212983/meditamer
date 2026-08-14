use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

const EXPECTED_CYCLES: u8 = 20;
const INTERNAL_FREE_FLOOR: u64 = 16_384;
const CPU0_STACK_FLOOR: u64 = 8_192;
const TOUCH_STACK_FLOOR: u64 = 1_024;
const MAX_POST_WARMUP_DRIFT: u64 = 1_024;

#[derive(Clone, Debug, Serialize)]
pub(super) struct Phase1dBaselineReport {
    pub artifact_elf_sha256: String,
    pub artifact_app_sha256: String,
    pub source_git_head: String,
    pub build_id: String,
    pub board_id: String,
    pub serial_port: String,
    pub cycles_expected: u8,
    pub active_cycles: usize,
    pub closed_cycles: usize,
    pub fault_latched_close_cycles: usize,
    pub after_cycles: usize,
    pub minimum_internal_free: Option<u64>,
    pub minimum_cpu0_stack: Option<u64>,
    pub minimum_touch_stack: Option<u64>,
    pub post_warmup_internal_drift: Option<u64>,
    pub opaque_internal_allocation_upper_bound: Option<u64>,
    pub baseline_passed: bool,
    pub phase1d_gate_passed: bool,
    pub violations: Vec<String>,
    pub remaining_gates: Vec<String>,
}

impl Phase1dBaselineReport {
    pub(super) fn empty() -> Self {
        Self {
            artifact_elf_sha256: String::new(),
            artifact_app_sha256: String::new(),
            source_git_head: String::new(),
            build_id: String::new(),
            board_id: String::new(),
            serial_port: String::new(),
            cycles_expected: EXPECTED_CYCLES,
            active_cycles: 0,
            closed_cycles: 0,
            fault_latched_close_cycles: 0,
            after_cycles: 0,
            minimum_internal_free: None,
            minimum_cpu0_stack: None,
            minimum_touch_stack: None,
            post_warmup_internal_drift: None,
            opaque_internal_allocation_upper_bound: None,
            baseline_passed: false,
            phase1d_gate_passed: false,
            violations: Vec::new(),
            remaining_gates: vec![
                "internal largest-block telemetry is unavailable".to_owned(),
                "forced close at both HCI TX await boundaries is not exercised".to_owned(),
                "forced close during active and full-queue RX callback ingress is not exercised"
                    .to_owned(),
            ],
        }
    }
}

fn fields(line: &str) -> BTreeMap<&str, &str> {
    line.split_ascii_whitespace()
        .filter_map(|token| token.split_once('='))
        .collect()
}

fn parse_u64(values: &BTreeMap<&str, &str>, key: &str) -> Option<u64> {
    values.get(key)?.parse().ok()
}

fn parse_cycle(values: &BTreeMap<&str, &str>) -> Option<u8> {
    parse_u64(values, "cycle").and_then(|value| u8::try_from(value).ok())
}

fn require_true(
    values: &BTreeMap<&str, &str>,
    key: &str,
    stage: &str,
    cycle: u8,
    violations: &mut Vec<String>,
) {
    if values.get(key) != Some(&"true") {
        violations.push(format!("{stage} cycle {cycle}: {key} is not true"));
    }
}

fn update_min(target: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        *target = Some(target.map_or(value, |current| current.min(value)));
    }
}

struct AnalysisState {
    report: Phase1dBaselineReport,
    active_cycles: BTreeSet<u8>,
    close_cycles: BTreeSet<u8>,
    after_internal: BTreeMap<u8, u64>,
    before_internal: Option<u64>,
    terminal_completed: bool,
}

impl AnalysisState {
    fn new() -> Self {
        Self {
            report: Phase1dBaselineReport::empty(),
            active_cycles: BTreeSet::new(),
            close_cycles: BTreeSet::new(),
            after_internal: BTreeMap::new(),
            before_internal: None,
            terminal_completed: false,
        }
    }

    fn observe_sample(&mut self, line: &str) {
        let values = fields(line);
        let stage = values.get("stage").copied().unwrap_or("missing");
        let cycle = parse_cycle(&values).unwrap_or(0);
        let internal = parse_u64(&values, "internal_free");
        match stage {
            "before" => self.before_internal = internal,
            "active" => {
                self.active_cycles.insert(cycle);
            }
            "after" => {
                if let Some(value) = internal {
                    self.after_internal.insert(cycle, value);
                }
            }
            _ => self
                .report
                .violations
                .push(format!("unknown sample stage {stage}")),
        }

        for key in [
            "coex",
            "wifi_controller",
            "net_runner",
            "wifi_link",
            "dhcp",
            "listener",
            "wifi_ok",
            "resource_ok",
        ] {
            require_true(&values, key, stage, cycle, &mut self.report.violations);
        }

        let cpu0 = parse_u64(&values, "cpu0_stack_min");
        let touch = parse_u64(&values, "touch_stack_min");
        update_min(&mut self.report.minimum_internal_free, internal);
        update_min(&mut self.report.minimum_cpu0_stack, cpu0);
        update_min(&mut self.report.minimum_touch_stack, touch);
        self.check_floor(stage, cycle, "internal_free", internal, INTERNAL_FREE_FLOOR);
        self.check_floor(stage, cycle, "cpu0_stack_min", cpu0, CPU0_STACK_FLOOR);
        self.check_floor(stage, cycle, "touch_stack_min", touch, TOUCH_STACK_FLOOR);
    }

    fn check_floor(&mut self, stage: &str, cycle: u8, label: &str, value: Option<u64>, floor: u64) {
        if value.is_none_or(|value| value < floor) {
            self.report
                .violations
                .push(format!("{stage} cycle {cycle}: {label} below {floor}"));
        }
    }

    fn observe_close(&mut self, line: &str) {
        let values = fields(line);
        let cycle = parse_cycle(&values).unwrap_or(0);
        self.close_cycles.insert(cycle);
        for (key, expected) in [
            ("settled_in_flight", "0"),
            ("packets_free", "4"),
            ("pool_exhausted", "0"),
            ("rx_queue_overflow", "0"),
            ("rx_oversize", "0"),
            ("tx_rejected", "0"),
            ("tx_timeout", "0"),
        ] {
            if values.get(key) != Some(&expected) {
                self.report.violations.push(format!(
                    "close cycle {cycle}: {key} expected {expected}, got {}",
                    values.get(key).copied().unwrap_or("missing")
                ));
            }
        }
        match values.get("transport_faulted") {
            Some(&"true") => self.report.fault_latched_close_cycles += 1,
            Some(&"false") => {}
            _ => self.report.violations.push(format!(
                "close cycle {cycle}: transport_faulted is missing or invalid"
            )),
        }
    }

    fn observe_terminal(&mut self, line: &str) {
        let values = fields(line);
        self.terminal_completed = values.get("state") == Some(&"completed")
            && values.get("cycle") == Some(&"20")
            && values.get("failure") == Some(&"none");
    }

    fn finish(mut self) -> Phase1dBaselineReport {
        let expected: BTreeSet<u8> = (1..=EXPECTED_CYCLES).collect();
        self.check_cycle_coverage(&expected);
        self.report.active_cycles = self.active_cycles.len();
        self.report.closed_cycles = self.close_cycles.len();
        self.report.after_cycles = self.after_internal.len();
        self.record_drift();
        if let (Some(before), Some(active_min)) =
            (self.before_internal, self.report.minimum_internal_free)
        {
            self.report.opaque_internal_allocation_upper_bound =
                Some(before.saturating_sub(active_min));
        }
        self.report.baseline_passed = self.report.violations.is_empty();
        self.report
    }

    fn check_cycle_coverage(&mut self, expected: &BTreeSet<u8>) {
        if &self.active_cycles != expected {
            self.report
                .violations
                .push("active samples do not cover cycles 1..20 exactly".to_owned());
        }
        if &self.close_cycles != expected {
            self.report
                .violations
                .push("close records do not cover cycles 1..20 exactly".to_owned());
        }
        let after_cycles = self.after_internal.keys().copied().collect::<BTreeSet<_>>();
        if &after_cycles != expected {
            self.report
                .violations
                .push("after samples do not cover cycles 1..20 exactly".to_owned());
        }
        if !self.terminal_completed {
            self.report
                .violations
                .push("missing successful completed terminal record".to_owned());
        }
    }

    fn record_drift(&mut self) {
        let (Some(warm), Some(last)) = (self.after_internal.get(&1), self.after_internal.get(&20))
        else {
            return;
        };
        let drift = warm.abs_diff(*last);
        self.report.post_warmup_internal_drift = Some(drift);
        if drift > MAX_POST_WARMUP_DRIFT {
            self.report.violations.push(format!(
                "post-warmup internal-free drift {drift} exceeds {MAX_POST_WARMUP_DRIFT}"
            ));
        }
    }
}

pub(super) fn analyze_lines(lines: &[String]) -> Phase1dBaselineReport {
    let mut state = AnalysisState::new();
    for line in lines {
        if line.starts_with("BLE_PHASE1D sample ") {
            state.observe_sample(line);
        } else if line.starts_with("BLE_PHASE1D close ") {
            state.observe_close(line);
        } else if line.starts_with("BLE_PHASE1D state=") {
            state.observe_terminal(line);
        }
    }
    state.finish()
}
