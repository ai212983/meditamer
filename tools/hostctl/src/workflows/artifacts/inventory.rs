//! Phase 0: read-only inventory of `logs/` -- totals, classifier output,
//! retention state, and due reviews. Never modifies the filesystem.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use chrono::Local;

use crate::logging::Logger;

use super::model::{Outcome, RetentionRecord};
use super::outcome::infer_unit_outcome;
use super::retention;
use super::scan::{self, TreeTotals};

const SEVEN_DAYS: Duration = Duration::from_secs(7 * 24 * 3600);
const THIRTY_DAYS: Duration = Duration::from_secs(30 * 24 * 3600);

pub struct RunUnitSummary {
    pub name: String,
    pub outcome: Outcome,
    pub age_days: f64,
    pub total_bytes: u64,
    pub payload_bytes: u64,
    pub payload_count: usize,
    pub payload_bytes_older_than_7d: u64,
    pub payload_count_older_than_7d: usize,
    pub retention: Option<RetentionRecord>,
}

pub struct StandaloneSummary {
    pub name: String,
    pub size_bytes: u64,
    pub age_days: f64,
    pub retention: Option<RetentionRecord>,
}

pub struct Inventory {
    pub tree_totals: TreeTotals,
    pub operational_bytes: u64,
    pub prune_report_bytes: u64,
    pub run_units: Vec<RunUnitSummary>,
    pub standalone_items: Vec<StandaloneSummary>,
}

impl Inventory {
    pub fn payload_bytes(&self) -> u64 {
        self.run_units.iter().map(|u| u.payload_bytes).sum()
    }

    pub fn payload_count(&self) -> usize {
        self.run_units.iter().map(|u| u.payload_count).sum()
    }

    pub fn payload_bytes_older_than_7d(&self) -> u64 {
        self.run_units
            .iter()
            .map(|u| u.payload_bytes_older_than_7d)
            .sum()
    }

    pub fn payload_count_older_than_7d(&self) -> usize {
        self.run_units
            .iter()
            .map(|u| u.payload_count_older_than_7d)
            .sum()
    }

    pub fn evidence_bytes(&self) -> u64 {
        let unit_bytes: u64 = self.run_units.iter().map(|u| u.total_bytes).sum();
        let standalone_bytes: u64 = self.standalone_items.iter().map(|s| s.size_bytes).sum();
        unit_bytes + standalone_bytes
    }
}

fn age_days(now: SystemTime, reference: SystemTime) -> f64 {
    now.duration_since(reference)
        .unwrap_or(Duration::ZERO)
        .as_secs_f64()
        / 86400.0
}

pub fn build_inventory(logs_root: &Path) -> Result<Inventory> {
    let now = SystemTime::now();
    let cutoff_7d = now
        .checked_sub(SEVEN_DAYS)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let cutoff_30d = now
        .checked_sub(THIRTY_DAYS)
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let tree_totals = scan::scan_tree_totals(logs_root, cutoff_30d)?;
    let operational_bytes = scan::EXCLUDED_ROOTS
        .iter()
        .filter(|&&name| name != ".prune-reports")
        .map(|name| {
            Ok::<u64, anyhow::Error>(
                scan::scan_tree_totals(&logs_root.join(name), cutoff_30d)?.total_bytes,
            )
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum();
    let prune_report_bytes =
        scan::scan_tree_totals(&logs_root.join(".prune-reports"), cutoff_30d)?.total_bytes;

    let mut run_units = Vec::new();
    for unit_dir in scan::list_run_units(logs_root)? {
        let name = unit_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let unit_scan = scan::scan_unit(&unit_dir)?;
        let unit_retention = retention::load_run_unit_retention(&unit_dir)?;

        let (outcome, reference_time) = infer_unit_outcome(&unit_dir, &unit_scan, now)?;

        let mut payload_bytes = 0u64;
        let mut payload_bytes_old = 0u64;
        let mut payload_count_old = 0usize;
        for payload in &unit_scan.payloads {
            payload_bytes += payload.size_bytes;
            if payload.modified <= cutoff_7d {
                payload_bytes_old += payload.size_bytes;
                payload_count_old += 1;
            }
        }

        run_units.push(RunUnitSummary {
            name,
            outcome,
            age_days: age_days(now, reference_time),
            total_bytes: unit_scan.total_bytes,
            payload_bytes,
            payload_count: unit_scan.payloads.len(),
            payload_bytes_older_than_7d: payload_bytes_old,
            payload_count_older_than_7d: payload_count_old,
            retention: unit_retention,
        });
    }

    let mut standalone_items = Vec::new();
    for path in scan::list_standalone_files(logs_root)? {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let metadata = fs::metadata(&path)?;
        let item_retention = retention::load_standalone_retention(&path)?;
        standalone_items.push(StandaloneSummary {
            name,
            size_bytes: metadata.len(),
            age_days: age_days(now, metadata.modified()?),
            retention: item_retention,
        });
    }

    Ok(Inventory {
        tree_totals,
        operational_bytes,
        prune_report_bytes,
        run_units,
        standalone_items,
    })
}

/// Formats a byte count as a human-scaled string (KiB/MiB/GiB).
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = UNITS[0];
    for candidate in &UNITS[1..] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = candidate;
    }
    if unit == "B" {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {unit}")
    }
}

pub fn print_inventory(logger: &mut Logger, inv: &Inventory) {
    let today = Local::now().date_naive();

    logger.info("Artifact inventory: logs/");
    logger.info(format!(
        "  total: {} across {} files (reconciles with `du`/`find` over logs/)",
        format_bytes(inv.tree_totals.total_bytes),
        inv.tree_totals.total_files
    ));
    logger.info(format!(
        "  files older than 30 days: {} / {}",
        inv.tree_totals.older_than_cutoff_count,
        format_bytes(inv.tree_totals.older_than_cutoff_bytes)
    ));
    logger.info(format!(
        "  operational (.state, locks): {}",
        format_bytes(inv.operational_bytes)
    ));
    logger.info(format!(
        "  prune reports (.prune-reports): {}",
        format_bytes(inv.prune_report_bytes)
    ));

    logger.info(format!(
        "  run units: {} ({} evidence)",
        inv.run_units.len(),
        format_bytes(inv.evidence_bytes())
    ));
    logger.info(format!(
        "  recognized flash payloads: {} / {}",
        inv.payload_count(),
        format_bytes(inv.payload_bytes())
    ));
    logger.info(format!(
        "  recognized flash payloads older than 7 days: {} / {}",
        inv.payload_count_older_than_7d(),
        format_bytes(inv.payload_bytes_older_than_7d())
    ));
    logger.info(format!(
        "  text logs and metadata (evidence - payloads): {}",
        format_bytes(inv.evidence_bytes().saturating_sub(inv.payload_bytes()))
    ));

    let passed = inv
        .run_units
        .iter()
        .filter(|u| u.outcome == Outcome::Passed)
        .count();
    let failed = inv
        .run_units
        .iter()
        .filter(|u| u.outcome == Outcome::Failed)
        .count();
    let inconclusive = inv
        .run_units
        .iter()
        .filter(|u| u.outcome == Outcome::Inconclusive)
        .count();
    logger.info(format!(
        "  outcomes: passed={passed} failed={failed} inconclusive={inconclusive}"
    ));
    logger.info(format!(
        "  standalone items: {} ({})",
        inv.standalone_items.len(),
        format_bytes(inv.standalone_items.iter().map(|s| s.size_bytes).sum())
    ));

    let retained_units: Vec<&RunUnitSummary> = inv
        .run_units
        .iter()
        .filter(|u| u.retention.is_some())
        .collect();
    if !retained_units.is_empty() {
        logger.info("  retained units:");
        for unit in &retained_units {
            let record = unit.retention.as_ref().expect("filtered on Some");
            logger.info(format!(
                "    {} outcome={} age={:.1}d scope={} owner={} review_after={} reason={}",
                unit.name,
                unit.outcome.label(),
                unit.age_days,
                record.scope.label(),
                record.owner,
                record.review_after,
                record.reason
            ));
        }
    }

    let mut due: Vec<(String, f64, &RetentionRecord)> = Vec::new();
    for unit in &inv.run_units {
        if let Some(record) = &unit.retention {
            if retention::is_review_due(record, today) {
                due.push((unit.name.clone(), unit.age_days, record));
            }
        }
    }
    for item in &inv.standalone_items {
        if let Some(record) = &item.retention {
            if retention::is_review_due(record, today) {
                due.push((item.name.clone(), item.age_days, record));
            }
        }
    }
    if !due.is_empty() {
        logger.info("  due for review:");
        for (name, age_days, record) in &due {
            logger.info(format!(
                "    {} age={:.1}d review_after={} owner={} reason={}",
                name, age_days, record.review_after, record.owner, record.reason
            ));
        }
    }
}
