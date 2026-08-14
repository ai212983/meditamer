//! The report a UI-lifecycle run produces, and the small numeric helpers the
//! analysis shares.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct UiLifecycleReport {
    pub(super) cycles_requested: u16,
    pub(super) max_baseline_drift_bytes: usize,
    pub(super) steps_expected: usize,
    pub(super) visible_surfaces: Vec<String>,
    pub(super) candidate_checkpoints: usize,
    pub(super) settled_checkpoints: usize,
    pub(super) settled_samples_by_surface: BTreeMap<u16, Vec<usize>>,
    pub(super) settled_total_by_surface: BTreeMap<u16, Vec<usize>>,
    pub(super) settled_used_blocks_by_surface: BTreeMap<u16, Vec<usize>>,
    pub(super) settled_frag_pct_by_surface: BTreeMap<u16, Vec<usize>>,
    pub(super) high_water_plateau_window_steps: usize,
    pub(super) high_water_plateau: bool,
    pub(super) current_heap_stable: bool,
    pub(super) max_transition_us: usize,
    pub(super) max_candidate_lvgl_used: usize,
    pub(super) min_cpu0_stack_headroom: Option<usize>,
    pub(super) min_internal_heap_free: Option<usize>,
    pub(super) min_external_heap_free: Option<usize>,
    pub(super) max_timer_gap_us: usize,
    pub(super) max_timer_runtime_us: usize,
    pub(super) violations: Vec<String>,
    pub(super) run_passed: bool,
}

pub(super) fn parse_usize_key(line: &str, key: &str) -> Option<usize> {
    line.split_ascii_whitespace()
        .find_map(|field| field.strip_prefix(key))?
        .trim_end_matches(|character: char| !character.is_ascii_digit())
        .parse()
        .ok()
}

pub(super) fn update_min(slot: &mut Option<usize>, value: usize) {
    *slot = Some(slot.map_or(value, |current| current.min(value)));
}

pub(super) fn sample_span(samples: &[usize]) -> usize {
    match (samples.iter().min(), samples.iter().max()) {
        (Some(minimum), Some(maximum)) => maximum - minimum,
        _ => 0,
    }
}

pub(super) fn all_equal(samples: &[usize]) -> bool {
    samples
        .first()
        .is_none_or(|first| samples.iter().all(|sample| sample == first))
}
