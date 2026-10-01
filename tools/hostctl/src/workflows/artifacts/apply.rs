//! Applies an artifact prune plan and maintains its audit report.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Local;
use serde::Serialize;

use crate::logging::Logger;

use super::inventory::format_bytes;
use super::prune::{relative_display, PrunePlan};

#[derive(Debug, Serialize)]
struct RemovedEntry {
    unit: String,
    role: &'static str,
    path: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
struct RemovedUnitEntry {
    unit: String,
    outcome: &'static str,
    age_days: f64,
    path: String,
    total_bytes: u64,
}

#[derive(Debug, Serialize)]
struct RemovedFileEntry {
    name: String,
    age_days: f64,
    path: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct PruneReport {
    pub generated_at: String,
    pub ignore_age: bool,
    pub runs: bool,
    pub min_age_days: u32,
    pub removed_count: usize,
    pub reclaimed_bytes: u64,
    removed: Vec<RemovedEntry>,
    pub removed_units_count: usize,
    pub units_reclaimed_bytes: u64,
    removed_units: Vec<RemovedUnitEntry>,
    pub removed_standalone_count: usize,
    pub standalone_reclaimed_bytes: u64,
    removed_standalone: Vec<RemovedFileEntry>,
    pub removed_prune_reports_count: usize,
    pub prune_reports_reclaimed_bytes: u64,
    removed_prune_reports: Vec<RemovedFileEntry>,
    pub status: &'static str,
    pub error: Option<String>,
}

pub fn apply_prune_plan(logs_root: &Path, plan: &PrunePlan) -> Result<(PruneReport, PathBuf)> {
    let report_path = prepare_report_path(logs_root)?;
    let mut report = PruneReport {
        generated_at: Local::now().to_rfc3339(),
        ignore_age: plan.ignore_age,
        runs: plan.runs,
        min_age_days: 7,
        removed_count: 0,
        reclaimed_bytes: 0,
        removed: Vec::with_capacity(plan.candidates.len()),
        removed_units_count: 0,
        units_reclaimed_bytes: 0,
        removed_units: Vec::with_capacity(plan.expired_units.len()),
        removed_standalone_count: 0,
        standalone_reclaimed_bytes: 0,
        removed_standalone: Vec::with_capacity(plan.expired_standalone.len()),
        removed_prune_reports_count: 0,
        prune_reports_reclaimed_bytes: 0,
        removed_prune_reports: Vec::with_capacity(plan.expired_reports.len()),
        status: "in_progress",
        error: None,
    };
    write_report(&report_path, &report)?;

    for candidate in &plan.candidates {
        if let Err(error) = fs::remove_file(&candidate.path)
            .with_context(|| format!("remove payload {}", candidate.path.display()))
        {
            return fail(report, report_path, error);
        }
        report.reclaimed_bytes += candidate.size_bytes;
        report.removed.push(RemovedEntry {
            unit: candidate.unit_name.clone(),
            role: candidate.role.label(),
            path: relative_display(&candidate.path),
            size_bytes: candidate.size_bytes,
        });
        report.removed_count = report.removed.len();
        write_report(&report_path, &report)?;
    }

    for unit in &plan.expired_units {
        if let Err(error) = fs::remove_dir_all(&unit.path)
            .with_context(|| format!("remove expired run unit {}", unit.path.display()))
        {
            return fail(report, report_path, error);
        }
        report.units_reclaimed_bytes += unit.total_bytes;
        report.removed_units.push(RemovedUnitEntry {
            unit: unit.name.clone(),
            outcome: unit.outcome.label(),
            age_days: unit.age_days,
            path: relative_display(&unit.path),
            total_bytes: unit.total_bytes,
        });
        report.removed_units_count = report.removed_units.len();
        write_report(&report_path, &report)?;
    }

    for item in &plan.expired_standalone {
        if let Err(error) = fs::remove_file(&item.path)
            .with_context(|| format!("remove expired standalone log {}", item.path.display()))
        {
            return fail(report, report_path, error);
        }
        report.standalone_reclaimed_bytes += item.size_bytes;
        report.removed_standalone.push(RemovedFileEntry {
            name: item.name.clone(),
            age_days: item.age_days,
            path: relative_display(&item.path),
            size_bytes: item.size_bytes,
        });
        report.removed_standalone_count = report.removed_standalone.len();
        write_report(&report_path, &report)?;
    }

    for item in &plan.expired_reports {
        if let Err(error) = fs::remove_file(&item.path)
            .with_context(|| format!("remove expired prune report {}", item.path.display()))
        {
            return fail(report, report_path, error);
        }
        report.prune_reports_reclaimed_bytes += item.size_bytes;
        report.removed_prune_reports.push(RemovedFileEntry {
            name: item.name.clone(),
            age_days: item.age_days,
            path: relative_display(&item.path),
            size_bytes: item.size_bytes,
        });
        report.removed_prune_reports_count = report.removed_prune_reports.len();
        write_report(&report_path, &report)?;
    }

    report.status = "completed";
    write_report(&report_path, &report)?;
    Ok((report, report_path))
}

fn prepare_report_path(logs_root: &Path) -> Result<PathBuf> {
    let reports_dir = logs_root.join(".prune-reports");
    fs::create_dir_all(&reports_dir)
        .with_context(|| format!("create prune report directory {}", reports_dir.display()))?;
    let now = Local::now();
    Ok(reports_dir.join(format!(
        "artifact-prune_{}_{:09}.json",
        now.format("%Y%m%d_%H%M%S"),
        now.timestamp_subsec_nanos()
    )))
}

fn write_report(path: &Path, report: &PruneReport) -> Result<()> {
    let body = serde_json::to_vec_pretty(report).context("serialize prune report")?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, body)
        .with_context(|| format!("write prune report {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("replace prune report {}", path.display()))
}

fn fail(
    mut report: PruneReport,
    report_path: PathBuf,
    error: anyhow::Error,
) -> Result<(PruneReport, PathBuf)> {
    report.status = "failed";
    report.error = Some(format!("{error:#}"));
    write_report(&report_path, &report)?;
    Err(error)
}

pub fn print_apply_result(logger: &mut Logger, report: &PruneReport, report_path: &Path) {
    let runs_summary = if report.runs {
        format!(
            " removed_units={} reclaimed_units={} removed_standalone={} reclaimed_standalone={} removed_prune_reports={} reclaimed_prune_reports={}",
            report.removed_units_count,
            format_bytes(report.units_reclaimed_bytes),
            report.removed_standalone_count,
            format_bytes(report.standalone_reclaimed_bytes),
            report.removed_prune_reports_count,
            format_bytes(report.prune_reports_reclaimed_bytes)
        )
    } else {
        String::new()
    };
    logger.info(format!(
        "Artifact prune applied: removed_payloads={} reclaimed_payloads={}{runs_summary} report={}",
        report.removed_count,
        format_bytes(report.reclaimed_bytes),
        relative_display(report_path)
    ));
}
