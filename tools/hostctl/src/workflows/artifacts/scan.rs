//! Discovers run units, standalone items, and recognized payloads under a
//! `logs/` root.
//!
//! A run unit is an immediate child directory of `logs/`; a standalone item
//! is an immediate child file. `logs/.state/`, `logs/locks/`, and
//! `logs/.prune-reports/` are operational/report roots, not evidence, and are
//! excluded from both.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

use super::model::{Payload, PayloadRole};

/// Top-level `logs/` children that are not run units or standalone evidence.
pub const EXCLUDED_ROOTS: &[&str] = &[".state", "locks", ".prune-reports"];

fn is_excluded_root(name: &str) -> bool {
    EXCLUDED_ROOTS.contains(&name)
}

/// Immediate child directories of `logs_root`, excluding operational roots.
pub fn list_run_units(logs_root: &Path) -> Result<Vec<PathBuf>> {
    let mut units = Vec::new();
    if !logs_root.is_dir() {
        return Ok(units);
    }
    for entry in fs::read_dir(logs_root)
        .with_context(|| format!("read logs root {}", logs_root.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if is_excluded_root(&name_str) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            units.push(entry.path());
        }
    }
    units.sort();
    Ok(units)
}

/// Immediate child files of `logs_root`, excluding adjacent `.retain.json`
/// retention records (those describe a sibling standalone item; they are not
/// independent evidence).
pub fn list_standalone_files(logs_root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !logs_root.is_dir() {
        return Ok(files);
    }
    for entry in fs::read_dir(logs_root)
        .with_context(|| format!("read logs root {}", logs_root.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !entry.file_type()?.is_file() {
            continue;
        }
        if name_str.ends_with(".retain.json") {
            continue;
        }
        files.push(entry.path());
    }
    files.sort();
    Ok(files)
}

/// Direct JSON reports written by artifact pruning. Nested files and other
/// file types are not part of report retention.
pub fn list_prune_reports(logs_root: &Path) -> Result<Vec<PathBuf>> {
    let reports_root = logs_root.join(".prune-reports");
    let mut reports = Vec::new();
    if !reports_root.is_dir() {
        return Ok(reports);
    }
    for entry in fs::read_dir(&reports_root)
        .with_context(|| format!("read prune reports root {}", reports_root.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        {
            reports.push(entry.path());
        }
    }
    reports.sort();
    Ok(reports)
}

/// Aggregate result of walking one run unit.
#[derive(Debug, Default)]
pub struct UnitScan {
    pub payloads: Vec<Payload>,
    pub total_bytes: u64,
    pub file_count: usize,
    pub newest_modified: Option<SystemTime>,
}

/// Recursively walks `unit_dir`, classifying recognized flash payloads and
/// accumulating totals. Recursion accommodates nested flash-capture layouts
/// (observed in legacy bundles); a directory only yields payloads when it
/// directly contains the current or known legacy flash-capture signature.
pub fn scan_unit(unit_dir: &Path) -> Result<UnitScan> {
    let mut scan = UnitScan::default();
    scan_dir(unit_dir, &mut scan)?;
    Ok(scan)
}

fn scan_dir(dir: &Path, scan: &mut UnitScan) -> Result<()> {
    let mut file_names = HashSet::new();
    let mut subdirs = Vec::new();

    for entry in fs::read_dir(dir).with_context(|| format!("read directory {}", dir.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            subdirs.push(entry.path());
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();

        // The retention record itself is metadata about the unit's
        // evidence, not evidence -- excluding it keeps "newest evidence-file
        // modification time" (the plan's run-age rule) from resetting every
        // time a `.retain.json` is added or renewed.
        if name != ".retain.json" {
            let metadata = entry.metadata()?;
            let modified = metadata.modified()?;
            scan.total_bytes += metadata.len();
            scan.file_count += 1;
            scan.newest_modified = Some(match scan.newest_modified {
                Some(existing) if existing >= modified => existing,
                _ => modified,
            });
        }

        file_names.insert(name);
    }

    let has_current_shape = file_names.contains("capture.log")
        && (file_names.contains("sha256.txt")
            || (file_names.contains("flash.log") && file_names.contains("summary.txt")));
    if has_current_shape {
        for role in PayloadRole::ALL {
            if !file_names.contains(role.file_name()) {
                continue;
            }
            let path = dir.join(role.file_name());
            let metadata =
                fs::metadata(&path).with_context(|| format!("stat payload {}", path.display()))?;
            scan.payloads.push(Payload {
                role,
                path,
                size_bytes: metadata.len(),
                modified: metadata.modified()?,
            });
        }
    }

    for subdir in subdirs {
        scan_dir(&subdir, scan)?;
    }
    Ok(())
}

/// Whole-tree totals for `root`, unconditionally including operational
/// roots -- this is the figure that reconciles with `du`/`find` over
/// `logs/`. `cutoff` buckets files whose modification time is at or before
/// it (used for the "files older than N days" inventory figure).
#[derive(Debug, Default)]
pub struct TreeTotals {
    pub total_bytes: u64,
    pub total_files: usize,
    pub older_than_cutoff_bytes: u64,
    pub older_than_cutoff_count: usize,
}

pub fn scan_tree_totals(root: &Path, cutoff: SystemTime) -> Result<TreeTotals> {
    let mut totals = TreeTotals::default();
    if root.is_dir() {
        scan_tree_dir(root, cutoff, &mut totals)?;
    }
    Ok(totals)
}

fn scan_tree_dir(dir: &Path, cutoff: SystemTime, totals: &mut TreeTotals) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read directory {}", dir.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            scan_tree_dir(&entry.path(), cutoff, totals)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let metadata = entry.metadata()?;
        let modified = metadata.modified()?;
        totals.total_bytes += metadata.len();
        totals.total_files += 1;
        if modified <= cutoff {
            totals.older_than_cutoff_bytes += metadata.len();
            totals.older_than_cutoff_count += 1;
        }
    }
    Ok(())
}
