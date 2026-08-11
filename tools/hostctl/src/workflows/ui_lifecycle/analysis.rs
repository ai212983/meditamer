//! Turning a captured device log into a [`UiLifecycleReport`].
//!
//! [`Scan`] accumulates what one pass over the log observed; the `observe_*`
//! methods each own one line shape, and the `check_*` methods each own one
//! expectation about the run as a whole.

use std::collections::BTreeMap;

use regex::Regex;

use super::report::{all_equal, parse_usize_key, sample_span, update_min, UiLifecycleReport};

/// Log markers that fail a run outright wherever they appear.
const FAILURE_MARKERS: [&str; 12] = [
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
];

/// What one pass over the log observed.
#[derive(Default)]
struct Scan {
    visible_surfaces: Vec<String>,
    settled_samples_by_surface: BTreeMap<u16, Vec<usize>>,
    settled_total_by_surface: BTreeMap<u16, Vec<usize>>,
    settled_used_blocks_by_surface: BTreeMap<u16, Vec<usize>>,
    settled_frag_pct_by_surface: BTreeMap<u16, Vec<usize>>,
    settled_high_water: Vec<usize>,
    settled_internal_heap_free: Vec<usize>,
    settled_external_heap_free: Vec<usize>,
    candidate_checkpoints: usize,
    settled_checkpoints: usize,
    violations: Vec<String>,
    max_transition_us: usize,
    max_candidate_lvgl_used: usize,
    min_cpu0_stack_headroom: Option<usize>,
    min_internal_heap_free: Option<usize>,
    min_external_heap_free: Option<usize>,
    max_timer_gap_us: usize,
    max_timer_runtime_us: usize,
}

impl Scan {
    fn violate(&mut self, message: String) {
        self.violations.push(message);
    }

    fn observe_visible(&mut self, line: &str, visible_re: &Regex) {
        if let Some(capture) = visible_re.captures(line) {
            self.visible_surfaces.push(capture[1].to_string());
        }
    }

    fn observe_candidate(&mut self, line: &str, index: usize) {
        if !line.contains("LVGL_LIFECYCLE phase=candidate_created") {
            return;
        }
        self.candidate_checkpoints += 1;
        match (
            parse_usize_key(line, "transition_us="),
            parse_usize_key(line, "lvgl_used="),
            parse_usize_key(line, "cpu0_stack_min="),
        ) {
            (Some(transition), Some(used), Some(stack)) => {
                self.max_transition_us = self.max_transition_us.max(transition);
                self.max_candidate_lvgl_used = self.max_candidate_lvgl_used.max(used);
                update_min(&mut self.min_cpu0_stack_headroom, stack);
            }
            _ => self.violate(format!(
                "line {} has a malformed candidate lifecycle checkpoint",
                index + 1
            )),
        }
    }

    fn observe_settled(&mut self, line: &str, index: usize, settled_re: &Regex) {
        if !line.contains("LVGL_LIFECYCLE phase=settled_after_delete") {
            return;
        }
        self.settled_checkpoints += 1;
        self.record_settled_sample(line, index, settled_re);
        self.record_settled_extrema(line, index);

        if let Some(value) = parse_usize_key(line, "heap_internal_free=") {
            self.settled_internal_heap_free.push(value);
        }
        if let Some(value) = parse_usize_key(line, "heap_external_free=") {
            self.settled_external_heap_free.push(value);
        }
    }

    fn record_settled_sample(&mut self, line: &str, index: usize, settled_re: &Regex) {
        let Some(capture) = settled_re.captures(line) else {
            self.violate(format!(
                "line {} has a malformed settled lifecycle checkpoint",
                index + 1
            ));
            return;
        };

        let surface = capture[1].parse::<u16>();
        let fields = (
            parse_usize_key(line, "lvgl_used="),
            parse_usize_key(line, "lvgl_total="),
            parse_usize_key(line, "lvgl_used_blocks="),
            parse_usize_key(line, "lvgl_frag_pct="),
            parse_usize_key(line, "lvgl_max_used="),
        );
        let (
            Ok(surface),
            (Some(used), Some(total), Some(used_blocks), Some(frag), Some(high_water)),
        ) = (surface, fields)
        else {
            self.violate(format!(
                "line {} has invalid settled lifecycle fields",
                index + 1
            ));
            return;
        };

        self.settled_samples_by_surface
            .entry(surface)
            .or_default()
            .push(used);
        self.settled_total_by_surface
            .entry(surface)
            .or_default()
            .push(total);
        self.settled_used_blocks_by_surface
            .entry(surface)
            .or_default()
            .push(used_blocks);
        self.settled_frag_pct_by_surface
            .entry(surface)
            .or_default()
            .push(frag);
        self.settled_high_water.push(high_water);
    }

    fn record_settled_extrema(&mut self, line: &str, index: usize) {
        for (key, maximum) in [
            ("transition_us=", &mut self.max_transition_us),
            ("timer_gap_max_us=", &mut self.max_timer_gap_us),
            ("timer_runtime_max_us=", &mut self.max_timer_runtime_us),
        ] {
            match parse_usize_key(line, key) {
                Some(value) => *maximum = (*maximum).max(value),
                None => self.violations.push(format!(
                    "line {} is missing lifecycle field `{}`",
                    index + 1,
                    key
                )),
            }
        }
        for (key, minimum) in [
            ("cpu0_stack_min=", &mut self.min_cpu0_stack_headroom),
            ("heap_internal_free=", &mut self.min_internal_heap_free),
            ("heap_external_free=", &mut self.min_external_heap_free),
        ] {
            match parse_usize_key(line, key) {
                Some(value) => update_min(minimum, value),
                None => self.violations.push(format!(
                    "line {} is missing lifecycle field `{}`",
                    index + 1,
                    key
                )),
            }
        }
    }

    fn observe_markers(&mut self, line: &str, index: usize) {
        for marker in FAILURE_MARKERS {
            if line.contains(marker) {
                self.violate(format!("line {} contains `{}`", index + 1, marker));
            }
        }
        if line.contains("LVGL_REFRESH phase=ui_cycle_step source=serial_ui_step")
            && (line.contains("kind=full") || line.contains("full_fallback=true"))
        {
            self.violate(format!(
                "line {} used an unexpected full refresh during UI cycling",
                index + 1
            ));
        }
    }

    fn check_route(&mut self, cycles: u16) {
        let expected_cycle = ["launcher", "diagnostics", "home"];
        let expected_surfaces = (0..cycles)
            .flat_map(|_| expected_cycle)
            .map(str::to_string)
            .collect::<Vec<_>>();
        if self.visible_surfaces != expected_surfaces {
            self.violate(format!(
                "visible route mismatch: expected {:?}, observed {:?}",
                expected_surfaces, self.visible_surfaces
            ));
        }
    }

    fn check_checkpoint_counts(&mut self, steps_expected: usize) {
        if self.candidate_checkpoints != steps_expected {
            self.violate(format!(
                "candidate checkpoint count {} != {}",
                self.candidate_checkpoints, steps_expected
            ));
        }
        if self.settled_checkpoints != steps_expected {
            self.violate(format!(
                "settled checkpoint count {} != {}",
                self.settled_checkpoints, steps_expected
            ));
        }
    }

    fn check_surface_baselines(&mut self, cycles: u16, max_baseline_drift_bytes: usize) {
        for (surface, expected_count) in [(1u16, cycles), (2, cycles), (3, cycles)] {
            let samples = self
                .settled_samples_by_surface
                .get(&surface)
                .cloned()
                .unwrap_or_default();
            if samples.len() != usize::from(expected_count) {
                self.violate(format!(
                    "surface {} settled samples {} != {}",
                    surface,
                    samples.len(),
                    expected_count
                ));
            }
            let used_span = sample_span(&samples);
            if used_span > max_baseline_drift_bytes {
                self.violate(format!(
                    "surface {} LVGL used baseline span {} exceeds {} bytes: {:?}",
                    surface, used_span, max_baseline_drift_bytes, samples
                ));
            }

            let total_samples = self
                .settled_total_by_surface
                .get(&surface)
                .cloned()
                .unwrap_or_default();
            let total_span = sample_span(&total_samples);
            if total_span > max_baseline_drift_bytes {
                self.violate(format!(
                    "surface {} LVGL usable-total span {} exceeds {} bytes: {:?}",
                    surface, total_span, max_baseline_drift_bytes, total_samples
                ));
            }

            let used_blocks = self
                .settled_used_blocks_by_surface
                .get(&surface)
                .cloned()
                .unwrap_or_default();
            if !all_equal(&used_blocks) {
                self.violate(format!(
                    "surface {} LVGL used-block count changed: {:?}",
                    surface, used_blocks
                ));
            }

            let frag_pct = self
                .settled_frag_pct_by_surface
                .get(&surface)
                .cloned()
                .unwrap_or_default();
            if sample_span(&frag_pct) > 1 {
                self.violate(format!(
                    "surface {} LVGL fragmentation varied by more than one point: {:?}",
                    surface, frag_pct
                ));
            }
        }
    }

    /// Returns the plateau window and whether the high-water mark held across it.
    fn check_high_water_plateau(&mut self, cycles: u16) -> (usize, bool) {
        let window = if cycles >= 10 {
            self.settled_high_water.len().min(30)
        } else {
            0
        };
        let plateau = window == 0
            || all_equal(&self.settled_high_water[self.settled_high_water.len() - window..]);
        if !plateau {
            self.violate(format!(
                "LVGL high-water did not plateau over the final {} transitions",
                window
            ));
        }
        (window, plateau)
    }

    fn check_heap_stability(&mut self) -> bool {
        let stable = all_equal(&self.settled_internal_heap_free)
            && all_equal(&self.settled_external_heap_free);
        if !stable {
            self.violate(
                "current internal or external heap free drifted during the run".to_string(),
            );
        }
        stable
    }
}

pub(super) fn analyze_lines(
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

    let mut scan = Scan::default();
    for (index, line) in lines.iter().enumerate() {
        scan.observe_visible(line, &visible_re);
        scan.observe_candidate(line, index);
        scan.observe_settled(line, index, &settled_re);
        scan.observe_markers(line, index);
    }

    scan.check_route(cycles);
    scan.check_checkpoint_counts(steps_expected);
    scan.check_surface_baselines(cycles, max_baseline_drift_bytes);
    let (high_water_plateau_window_steps, high_water_plateau) =
        scan.check_high_water_plateau(cycles);
    let current_heap_stable = scan.check_heap_stability();

    UiLifecycleReport {
        cycles_requested: cycles,
        max_baseline_drift_bytes,
        steps_expected,
        visible_surfaces: scan.visible_surfaces,
        candidate_checkpoints: scan.candidate_checkpoints,
        settled_checkpoints: scan.settled_checkpoints,
        settled_samples_by_surface: scan.settled_samples_by_surface,
        settled_total_by_surface: scan.settled_total_by_surface,
        settled_used_blocks_by_surface: scan.settled_used_blocks_by_surface,
        settled_frag_pct_by_surface: scan.settled_frag_pct_by_surface,
        high_water_plateau_window_steps,
        high_water_plateau,
        current_heap_stable,
        max_transition_us: scan.max_transition_us,
        max_candidate_lvgl_used: scan.max_candidate_lvgl_used,
        min_cpu0_stack_headroom: scan.min_cpu0_stack_headroom,
        min_internal_heap_free: scan.min_internal_heap_free,
        min_external_heap_free: scan.min_external_heap_free,
        max_timer_gap_us: scan.max_timer_gap_us,
        max_timer_runtime_us: scan.max_timer_runtime_us,
        run_passed: scan.violations.is_empty(),
        violations: scan.violations,
    }
}
