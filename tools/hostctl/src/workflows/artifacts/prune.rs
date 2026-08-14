//! Phase 1 + Phase 2: recognized-payload thinning and, with `--runs`,
//! whole-run/standalone-log expiry. A unit selected for whole-unit expiry
//! is removed outright and is not also emitted as separate payload
//! candidates -- removing its directory removes its payloads too.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use crate::logging::Logger;
use crate::workflows::common::repo_root;

use super::expiry::{self, ExpiredPruneReport, ExpiredStandalone, ExpiredUnit};
use super::inventory::format_bytes;
use super::model::{PayloadRole, RetentionScope};
use super::outcome::infer_unit_outcome;
use super::retention;
use super::scan;

const SEVEN_DAYS: Duration = Duration::from_secs(7 * 24 * 3600);

/// Displays `path` relative to the repo root when possible, matching the
/// repo-relative-path convention other generated reports use (e.g. the
/// Wi-Fi regression gate's `report.json`), even though `logs/` is
/// gitignored. Falls back to the absolute path for inputs outside the repo
/// root (e.g. test fixtures under a tempdir).
pub(super) fn relative_display(path: &Path) -> String {
    match path.strip_prefix(repo_root()) {
        Ok(relative) => relative.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

fn age_days(now: SystemTime, reference: SystemTime) -> f64 {
    now.duration_since(reference)
        .unwrap_or(Duration::ZERO)
        .as_secs_f64()
        / 86400.0
}

fn sort_candidates_by_path(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| a.path.cmp(&b.path));
}

fn sort_expired_units_by_path(units: &mut [ExpiredUnit]) {
    units.sort_by(|a, b| a.path.cmp(&b.path));
}

fn sort_expired_standalone_by_path(items: &mut [ExpiredStandalone]) {
    items.sort_by(|a, b| a.path.cmp(&b.path));
}

fn sort_expired_reports_by_path(items: &mut [ExpiredPruneReport]) {
    items.sort_by(|a, b| a.path.cmp(&b.path));
}

pub struct PruneOptions {
    pub ignore_age: bool,
    pub runs: bool,
}

pub struct Candidate {
    pub unit_name: String,
    pub role: PayloadRole,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub age_days: f64,
    pub unit_retention_scope: Option<RetentionScope>,
}

pub struct PrunePlan {
    pub candidates: Vec<Candidate>,
    pub retained_payload_count: usize,
    pub ignore_age: bool,
    pub runs: bool,
    pub expired_units: Vec<ExpiredUnit>,
    pub retained_unit_count: usize,
    pub expired_standalone: Vec<ExpiredStandalone>,
    pub retained_standalone_count: usize,
    pub expired_reports: Vec<ExpiredPruneReport>,
}

impl PrunePlan {
    pub fn reclaimable_bytes(&self) -> u64 {
        self.candidates.iter().map(|c| c.size_bytes).sum()
    }

    pub fn units_reclaimable_bytes(&self) -> u64 {
        self.expired_units.iter().map(|u| u.total_bytes).sum()
    }

    pub fn standalone_reclaimable_bytes(&self) -> u64 {
        self.expired_standalone.iter().map(|s| s.size_bytes).sum()
    }

    pub fn total_reclaimable_bytes(&self) -> u64 {
        self.reclaimable_bytes()
            + self.units_reclaimable_bytes()
            + self.standalone_reclaimable_bytes()
            + self.reports_reclaimable_bytes()
    }

    pub fn reports_reclaimable_bytes(&self) -> u64 {
        self.expired_reports.iter().map(|r| r.size_bytes).sum()
    }

    fn assemble(units: RunUnitsResult, standalone: StandaloneResult, opts: &PruneOptions) -> Self {
        PrunePlan {
            candidates: units.candidates,
            retained_payload_count: units.retained_payload_count,
            ignore_age: opts.ignore_age,
            runs: opts.runs,
            expired_units: units.expired_units,
            retained_unit_count: units.retained_unit_count,
            expired_standalone: standalone.expired,
            retained_standalone_count: standalone.retained_count,
            expired_reports: Vec::new(),
        }
    }
}

/// Selects recognized, unretained payloads eligible for removal, and --
/// with `runs` -- whole run units and standalone logs past their
/// outcome-based age. With `ignore_age`, all three minimum ages are
/// suppressed; retention and selection logic are otherwise unchanged. A
/// unit selected for whole-unit expiry is not also scanned for individual
/// payload candidates.
pub fn build_prune_plan(
    logs_root: &Path,
    opts: &PruneOptions,
    now: SystemTime,
) -> Result<PrunePlan> {
    let payload_cutoff = now
        .checked_sub(SEVEN_DAYS)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut units = collect_run_units(logs_root, opts, now, payload_cutoff)?;
    let mut standalone = collect_standalone(logs_root, opts, now)?;
    let mut expired_reports = collect_expired_reports(logs_root, opts, now)?;

    sort_candidates_by_path(&mut units.candidates);
    sort_expired_units_by_path(&mut units.expired_units);
    sort_expired_standalone_by_path(&mut standalone.expired);
    sort_expired_reports_by_path(&mut expired_reports);

    let mut plan = PrunePlan::assemble(units, standalone, opts);
    plan.expired_reports = expired_reports;
    Ok(plan)
}

fn collect_expired_reports(
    logs_root: &Path,
    opts: &PruneOptions,
    now: SystemTime,
) -> Result<Vec<ExpiredPruneReport>> {
    if !opts.runs {
        return Ok(Vec::new());
    }
    let cutoff = now
        .checked_sub(expiry::PRUNE_REPORT_MAX_AGE)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut expired = Vec::new();
    for path in scan::list_prune_reports(logs_root)? {
        let metadata =
            fs::metadata(&path).with_context(|| format!("stat prune report {}", path.display()))?;
        let modified = metadata.modified()?;
        if !opts.ignore_age && modified > cutoff {
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        expired.push(ExpiredPruneReport {
            name,
            path,
            age_days: age_days(now, modified),
            size_bytes: metadata.len(),
        });
    }
    Ok(expired)
}

/// Accumulated result of walking every run unit once.
#[derive(Default)]
struct RunUnitsResult {
    candidates: Vec<Candidate>,
    retained_payload_count: usize,
    expired_units: Vec<ExpiredUnit>,
    retained_unit_count: usize,
}

/// Walks every run unit once, evaluating whole-unit expiry (when `runs` is
/// set) and payload thinning for units that survive it.
fn collect_run_units(
    logs_root: &Path,
    opts: &PruneOptions,
    now: SystemTime,
    payload_cutoff: SystemTime,
) -> Result<RunUnitsResult> {
    let mut result = RunUnitsResult::default();

    for unit_dir in scan::list_run_units(logs_root)? {
        let unit = evaluate_unit(unit_dir, opts, now, payload_cutoff)?;
        match unit.expired {
            Some(expired) => result.expired_units.push(expired),
            None => {
                if unit.retained_from_expiry {
                    result.retained_unit_count += 1;
                }
                result.retained_payload_count += unit.retained_payload_count;
                result.candidates.extend(unit.candidates);
            }
        }
    }

    Ok(result)
}

/// One run unit's evaluation result: either it expired outright (whole-unit
/// removal, payloads not separately scanned), or it survives this pass with
/// zero or more individual payload candidates.
struct UnitEvaluation {
    expired: Option<ExpiredUnit>,
    retained_from_expiry: bool,
    candidates: Vec<Candidate>,
    retained_payload_count: usize,
}

fn evaluate_unit(
    unit_dir: PathBuf,
    opts: &PruneOptions,
    now: SystemTime,
    payload_cutoff: SystemTime,
) -> Result<UnitEvaluation> {
    let unit_name = unit_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unit_scan = scan::scan_unit(&unit_dir)?;
    let unit_retention = retention::load_run_unit_retention(&unit_dir)?;

    let mut retained_from_expiry = false;
    if opts.runs {
        if let Some(expired) = evaluate_unit_expiry(
            &unit_dir,
            unit_name.clone(),
            &unit_scan,
            unit_retention.is_some(),
            opts,
            now,
        )? {
            return Ok(UnitEvaluation {
                expired: Some(expired),
                retained_from_expiry: false,
                candidates: Vec::new(),
                retained_payload_count: 0,
            });
        }
        retained_from_expiry = unit_retention.is_some();
    }

    let mut candidates = Vec::new();
    let mut retained_payload_count = 0usize;
    for payload in unit_scan.payloads {
        let old_enough = opts.ignore_age || payload.modified <= payload_cutoff;
        if !old_enough {
            continue;
        }

        let protected = unit_retention
            .as_ref()
            .is_some_and(|record| payload.role.protected_by(record.scope));
        if protected {
            retained_payload_count += 1;
            continue;
        }

        candidates.push(Candidate {
            unit_name: unit_name.clone(),
            role: payload.role,
            path: payload.path,
            size_bytes: payload.size_bytes,
            age_days: age_days(now, payload.modified),
            unit_retention_scope: unit_retention.as_ref().map(|r| r.scope),
        });
    }

    Ok(UnitEvaluation {
        expired: None,
        retained_from_expiry,
        candidates,
        retained_payload_count,
    })
}

/// Returns `Some(ExpiredUnit)` when `unit_dir` is old enough (per its
/// outcome-based threshold) and unretained; `None` when it is too young, or
/// old enough but retained (the caller still runs payload thinning either
/// way).
fn evaluate_unit_expiry(
    unit_dir: &Path,
    unit_name: String,
    unit_scan: &scan::UnitScan,
    is_retained: bool,
    opts: &PruneOptions,
    now: SystemTime,
) -> Result<Option<ExpiredUnit>> {
    let (outcome, reference_time) = infer_unit_outcome(unit_dir, unit_scan, now)?;
    let unit_cutoff = now
        .checked_sub(expiry::unit_max_age(outcome))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let old_enough = opts.ignore_age || reference_time <= unit_cutoff;

    if !old_enough || is_retained {
        return Ok(None);
    }

    Ok(Some(ExpiredUnit {
        name: unit_name,
        path: unit_dir.to_path_buf(),
        outcome,
        age_days: age_days(now, reference_time),
        total_bytes: unit_scan.total_bytes,
    }))
}

/// Accumulated result of scanning standalone items for expiry.
#[derive(Default)]
struct StandaloneResult {
    expired: Vec<ExpiredStandalone>,
    retained_count: usize,
}

/// Selects expired standalone logs; a no-op unless `opts.runs` is set.
fn collect_standalone(
    logs_root: &Path,
    opts: &PruneOptions,
    now: SystemTime,
) -> Result<StandaloneResult> {
    let mut result = StandaloneResult::default();
    if !opts.runs {
        return Ok(result);
    }

    let standalone_cutoff = now
        .checked_sub(expiry::STANDALONE_LOG_MAX_AGE)
        .unwrap_or(SystemTime::UNIX_EPOCH);

    for path in scan::list_standalone_files(logs_root)? {
        evaluate_standalone(path, opts, now, standalone_cutoff, &mut result)?;
    }

    Ok(result)
}

fn evaluate_standalone(
    path: PathBuf,
    opts: &PruneOptions,
    now: SystemTime,
    standalone_cutoff: SystemTime,
    result: &mut StandaloneResult,
) -> Result<()> {
    let metadata =
        fs::metadata(&path).with_context(|| format!("stat standalone item {}", path.display()))?;
    let modified = metadata.modified()?;
    let old_enough = opts.ignore_age || modified <= standalone_cutoff;
    if !old_enough {
        return Ok(());
    }

    if retention::load_standalone_retention(&path)?.is_some() {
        result.retained_count += 1;
        return Ok(());
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    result.expired.push(ExpiredStandalone {
        name,
        path,
        age_days: age_days(now, modified),
        size_bytes: metadata.len(),
    });
    Ok(())
}

pub fn print_prune_plan(logger: &mut Logger, plan: &PrunePlan) {
    if plan.candidates.is_empty() {
        logger.info("Artifact prune: no eligible payloads.");
    } else {
        logger.info("Artifact prune candidates:");
        for candidate in &plan.candidates {
            let reason = if plan.ignore_age {
                "ignore-age".to_string()
            } else {
                format!("age={:.1}d>=7d", candidate.age_days)
            };
            let retained_scope = candidate
                .unit_retention_scope
                .map(|scope| scope.label())
                .unwrap_or("none");
            logger.info(format!(
                "  {} [{}] {} reason={reason} retained_scope={retained_scope} size={}",
                candidate.unit_name,
                candidate.role.label(),
                relative_display(&candidate.path),
                format_bytes(candidate.size_bytes)
            ));
        }
    }
    logger.info(format!(
        "  candidates={} reclaimable={} retained_payloads_skipped={}",
        plan.candidates.len(),
        format_bytes(plan.reclaimable_bytes()),
        plan.retained_payload_count
    ));

    if !plan.runs {
        return;
    }

    if plan.expired_units.is_empty() {
        logger.info("Artifact prune: no eligible run units.");
    } else {
        logger.info("Expired run units:");
        for unit in &plan.expired_units {
            let max_age_days = expiry::unit_max_age(unit.outcome).as_secs_f64() / 86400.0;
            let reason = if plan.ignore_age {
                "ignore-age".to_string()
            } else {
                format!("age={:.1}d>={max_age_days:.0}d", unit.age_days)
            };
            logger.info(format!(
                "  {} outcome={} {} reason={reason} size={}",
                unit.name,
                unit.outcome.label(),
                relative_display(&unit.path),
                format_bytes(unit.total_bytes)
            ));
        }
    }
    logger.info(format!(
        "  expired_units={} reclaimable={} retained_units_skipped={}",
        plan.expired_units.len(),
        format_bytes(plan.units_reclaimable_bytes()),
        plan.retained_unit_count
    ));

    if plan.expired_standalone.is_empty() {
        logger.info("Artifact prune: no eligible standalone logs.");
    } else {
        logger.info("Expired standalone logs:");
        for item in &plan.expired_standalone {
            let reason = if plan.ignore_age {
                "ignore-age".to_string()
            } else {
                format!("age={:.1}d>=30d", item.age_days)
            };
            logger.info(format!(
                "  {} {} reason={reason} size={}",
                item.name,
                relative_display(&item.path),
                format_bytes(item.size_bytes)
            ));
        }
    }
    logger.info(format!(
        "  expired_standalone={} reclaimable={} retained_standalone_skipped={}",
        plan.expired_standalone.len(),
        format_bytes(plan.standalone_reclaimable_bytes()),
        plan.retained_standalone_count
    ));
    if plan.expired_reports.is_empty() {
        logger.info("Artifact prune: no eligible prune reports.");
    } else {
        logger.info("Expired prune reports:");
        for report in &plan.expired_reports {
            let reason = if plan.ignore_age {
                "ignore-age".to_string()
            } else {
                format!("age={:.1}d>=90d", report.age_days)
            };
            logger.info(format!(
                "  {} {} reason={reason} size={}",
                report.name,
                relative_display(&report.path),
                format_bytes(report.size_bytes)
            ));
        }
    }
    logger.info(format!(
        "  expired_prune_reports={} reclaimable={}",
        plan.expired_reports.len(),
        format_bytes(plan.reports_reclaimable_bytes())
    ));
    logger.info(format!(
        "  total reclaimable (payloads + units + standalone + prune reports)={}",
        format_bytes(plan.total_reclaimable_bytes())
    ));
}
