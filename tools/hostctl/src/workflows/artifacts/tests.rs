//! Fixture-driven coverage for the classifier, retention handling, and
//! prune dry-run/apply/idempotency behavior described by
//! `docs/archive/host-tooling/log-and-artifact-pruning.md` (Phase 0/1 R-001, and Phase 2
//! run/standalone expiry).

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use tempfile::tempdir;

use super::apply::apply_prune_plan;
use super::inventory::build_inventory;
use super::model::Outcome;
use super::prune::{build_prune_plan, PruneOptions};
use super::scan::list_run_units;

const ROLES: [&str; 4] = [
    "firmware.elf",
    "app.bin",
    "bootloader.bin",
    "partition-table.bin",
];

fn touch_with_age(path: &Path, days_ago: u64) -> Result<()> {
    let file = File::options().write(true).open(path)?;
    let modified = SystemTime::now() - Duration::from_secs(days_ago * 24 * 3600);
    file.set_modified(modified)?;
    Ok(())
}

fn touch_at(path: &Path, modified: SystemTime) -> Result<()> {
    File::options()
        .write(true)
        .open(path)?
        .set_modified(modified)?;
    Ok(())
}

/// Writes a flash-capture-shaped bundle: sibling markers plus the given
/// payload roles, all backdated to `days_ago`.
fn write_flash_bundle(dir: &Path, roles: &[&str], days_ago: u64) -> Result<()> {
    write_flash_bundle_at(
        dir,
        roles,
        SystemTime::now() - Duration::from_secs(days_ago * 24 * 3600),
    )
}

fn write_flash_bundle_at(dir: &Path, roles: &[&str], modified: SystemTime) -> Result<()> {
    fs::create_dir_all(dir)?;
    for marker in [
        "flash.log",
        "capture.log",
        "summary.txt",
        "sha256.txt",
        "build-metadata.txt",
    ] {
        let path = dir.join(marker);
        fs::write(&path, "marker")?;
        touch_at(&path, modified)?;
    }
    for role in roles {
        let path = dir.join(role);
        fs::write(&path, format!("payload:{role}"))?;
        touch_at(&path, modified)?;
    }
    Ok(())
}

fn write_retain_json(path: &Path, scope: &str, review_after: &str) -> Result<()> {
    fs::write(
        path,
        format!(
            r#"{{"scope":"{scope}","reason":"fixture","owner":"fixture-owner","review_after":"{review_after}"}}"#
        ),
    )?;
    Ok(())
}

/// Writes a recognized `report.json` (the Wi-Fi regression gate shape) with
/// `finished_at` backdated `days_ago` days, driving the unit's outcome/age
/// independent of any file mtimes in the directory.
fn write_report(dir: &Path, final_status: &str, days_ago: i64) -> Result<()> {
    let completed = SystemTime::now() - Duration::from_secs(days_ago as u64 * 24 * 3600);
    write_report_at(dir, final_status, completed)
}

fn write_report_at(dir: &Path, final_status: &str, completed: SystemTime) -> Result<()> {
    let finished_at =
        chrono::DateTime::<Utc>::from(completed).to_rfc3339_opts(SecondsFormat::Secs, true);
    let path = dir.join("report.json");
    fs::write(
        &path,
        format!(r#"{{"final_status":"{final_status}","finished_at":"{finished_at}"}}"#),
    )?;
    touch_at(&path, completed)?;
    Ok(())
}

fn write_standalone(logs_root: &Path, name: &str, days_ago: u64) -> Result<PathBuf> {
    let path = logs_root.join(name);
    fs::write(&path, "log content")?;
    touch_with_age(&path, days_ago)?;
    Ok(path)
}

#[test]
fn classifier_recognizes_flash_capture_layout_only() -> Result<()> {
    let logs_root = tempdir()?;

    // Genuine flash-capture bundle: recognized.
    write_flash_bundle(&logs_root.path().join("flash_capture_a"), &ROLES, 0)?;

    // Same filenames but no sibling markers: legacy/unclassified, not recognized.
    let legacy = logs_root.path().join("legacy_no_marker");
    fs::create_dir_all(&legacy)?;
    fs::write(legacy.join("app.bin"), "not-a-real-payload")?;

    // A generic command summary next to a coincidentally named binary is
    // still not a flash-capture output.
    let summary_only = logs_root.path().join("summary_only");
    fs::create_dir_all(&summary_only)?;
    fs::write(summary_only.join("app.bin"), "not-a-real-payload")?;
    fs::write(summary_only.join("summary.txt"), "not-a-capture")?;

    let inv = build_inventory(logs_root.path())?;
    let recognized_unit = inv
        .run_units
        .iter()
        .find(|u| u.name == "flash_capture_a")
        .expect("recognized unit present");
    assert_eq!(recognized_unit.payload_count, 4);

    let legacy_unit = inv
        .run_units
        .iter()
        .find(|u| u.name == "legacy_no_marker")
        .expect("legacy unit present");
    assert_eq!(legacy_unit.payload_count, 0);
    let summary_only_unit = inv
        .run_units
        .iter()
        .find(|u| u.name == "summary_only")
        .expect("summary-only unit present");
    assert_eq!(summary_only_unit.payload_count, 0);

    Ok(())
}

#[test]
fn classifier_recurses_into_nested_bundle_layouts() -> Result<()> {
    let logs_root = tempdir()?;
    // Mirrors a real observed shape: an outer unit whose "capture.log" is a
    // directory containing a full nested bundle.
    let outer = logs_root.path().join("nested_unit");
    fs::create_dir_all(&outer)?;
    fs::write(outer.join("flash.log"), "outer")?;
    write_flash_bundle(&outer.join("capture.log"), &["app.bin"], 0)?;

    let inv = build_inventory(logs_root.path())?;
    let unit = inv
        .run_units
        .iter()
        .find(|u| u.name == "nested_unit")
        .expect("unit present");
    assert_eq!(unit.payload_count, 1);

    Ok(())
}

#[test]
fn partially_thinned_unit_scans_remaining_payloads_only() -> Result<()> {
    let logs_root = tempdir()?;
    write_flash_bundle(&logs_root.path().join("partial"), &["app.bin"], 10)?;

    let inv = build_inventory(logs_root.path())?;
    let unit = inv
        .run_units
        .iter()
        .find(|u| u.name == "partial")
        .expect("unit present");
    assert_eq!(unit.payload_count, 1);

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert_eq!(plan.candidates.len(), 1);
    assert_eq!(plan.candidates[0].role.label(), "app_bin");

    Ok(())
}

#[test]
fn operational_roots_are_excluded_from_run_units() -> Result<()> {
    let logs_root = tempdir()?;
    fs::create_dir_all(logs_root.path().join(".state"))?;
    fs::create_dir_all(logs_root.path().join("locks"))?;
    write_flash_bundle(&logs_root.path().join("real_unit"), &ROLES, 0)?;

    let units = list_run_units(logs_root.path())?;
    let names: Vec<String> = units
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["real_unit".to_string()]);

    Ok(())
}

#[test]
fn dry_run_excludes_recent_unretained_payloads_by_default() -> Result<()> {
    let logs_root = tempdir()?;
    write_flash_bundle(&logs_root.path().join("recent"), &ROLES, 0)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert!(plan.candidates.is_empty());

    Ok(())
}

#[test]
fn dry_run_includes_expired_unretained_payloads() -> Result<()> {
    let logs_root = tempdir()?;
    write_flash_bundle(&logs_root.path().join("expired"), &ROLES, 10)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert_eq!(plan.candidates.len(), 4);
    assert_eq!(
        plan.reclaimable_bytes(),
        plan.candidates.iter().map(|c| c.size_bytes).sum::<u64>()
    );

    Ok(())
}

#[test]
fn ignore_age_includes_recent_unretained_payloads() -> Result<()> {
    let logs_root = tempdir()?;
    write_flash_bundle(&logs_root.path().join("recent"), &ROLES, 0)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: true,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert_eq!(plan.candidates.len(), 4);

    Ok(())
}

#[test]
fn ignore_age_still_honors_retention() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("recent_retained_debug");
    write_flash_bundle(&unit, &ROLES, 0)?;
    write_retain_json(&unit.join(".retain.json"), "debug", "2099-01-01")?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: true,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert!(plan.candidates.is_empty());
    assert_eq!(plan.retained_payload_count, 4);

    Ok(())
}

#[test]
fn reflash_scope_protects_app_bootloader_partition_but_not_elf() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("retained_reflash");
    write_flash_bundle(&unit, &ROLES, 10)?;
    write_retain_json(&unit.join(".retain.json"), "reflash", "2099-01-01")?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert_eq!(plan.candidates.len(), 1);
    assert_eq!(plan.candidates[0].role.label(), "firmware_elf");
    assert_eq!(plan.retained_payload_count, 3);

    Ok(())
}

#[test]
fn debug_scope_protects_all_recognized_roles() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("retained_debug");
    write_flash_bundle(&unit, &ROLES, 10)?;
    write_retain_json(&unit.join(".retain.json"), "debug", "2099-01-01")?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert!(plan.candidates.is_empty());
    assert_eq!(plan.retained_payload_count, 4);

    Ok(())
}

#[test]
fn evidence_scope_does_not_protect_payloads_from_thinning() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("retained_evidence");
    write_flash_bundle(&unit, &ROLES, 10)?;
    write_retain_json(&unit.join(".retain.json"), "evidence", "2099-01-01")?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert_eq!(plan.candidates.len(), 4);
    assert_eq!(plan.retained_payload_count, 0);

    Ok(())
}

#[test]
fn apply_removes_exactly_the_dry_run_candidate_set() -> Result<()> {
    let logs_root = tempdir()?;
    write_flash_bundle(&logs_root.path().join("expired"), &ROLES, 10)?;

    let dry_run_plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    let dry_run_paths: Vec<PathBuf> = dry_run_plan
        .candidates
        .iter()
        .map(|c| c.path.clone())
        .collect();

    let (report, report_path) = apply_prune_plan(logs_root.path(), &dry_run_plan)?;
    assert_eq!(report.removed_count, dry_run_paths.len());
    assert!(report_path.starts_with(logs_root.path().join(".prune-reports")));
    for path in &dry_run_paths {
        assert!(
            !path.exists(),
            "{} should have been removed",
            path.display()
        );
    }

    Ok(())
}

#[test]
fn second_apply_is_idempotent() -> Result<()> {
    let logs_root = tempdir()?;
    write_flash_bundle(&logs_root.path().join("expired"), &ROLES, 10)?;

    let first_plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert_eq!(first_plan.candidates.len(), 4);
    let (first_report, _) = apply_prune_plan(logs_root.path(), &first_plan)?;
    assert_eq!(first_report.reclaimed_bytes, first_plan.reclaimable_bytes());
    assert!(first_report.reclaimed_bytes > 0);

    let second_plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert!(second_plan.candidates.is_empty());
    let (second_report, _) = apply_prune_plan(logs_root.path(), &second_plan)?;
    assert_eq!(second_report.removed_count, 0);
    assert_eq!(second_report.reclaimed_bytes, 0);

    Ok(())
}

#[test]
fn standalone_retention_is_reported_and_due_reviews_surface() -> Result<()> {
    let logs_root = tempdir()?;
    let standalone = logs_root.path().join("old_run.log");
    fs::write(&standalone, "log content")?;
    touch_with_age(&standalone, 40)?;
    let retain_path = PathBuf::from(format!("{}.retain.json", standalone.display()));
    write_retain_json(&retain_path, "evidence", "2020-01-01")?;

    let inv = build_inventory(logs_root.path())?;
    let item = inv
        .standalone_items
        .iter()
        .find(|s| s.name == "old_run.log")
        .expect("standalone item present");
    assert!(item.retention.is_some());
    assert!(item.age_days >= 39.0);

    Ok(())
}

#[test]
fn inconclusive_outcome_when_no_recognized_report_present() -> Result<()> {
    let logs_root = tempdir()?;
    write_flash_bundle(&logs_root.path().join("flash_only"), &ROLES, 0)?;

    let inv = build_inventory(logs_root.path())?;
    let unit = inv
        .run_units
        .iter()
        .find(|u| u.name == "flash_only")
        .expect("unit present");
    assert_eq!(unit.outcome, Outcome::Inconclusive);

    Ok(())
}

#[test]
fn recognized_report_drives_outcome() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root
        .path()
        .join("wifi_regression_gate_20260101_000000");
    fs::create_dir_all(&unit)?;
    fs::write(
        unit.join("report.json"),
        r#"{"final_status":"passed","finished_at":"2026-01-01T00:05:00Z"}"#,
    )?;

    let inv = build_inventory(logs_root.path())?;
    let summary = inv
        .run_units
        .iter()
        .find(|u| u.name == "wifi_regression_gate_20260101_000000")
        .expect("unit present");
    assert_eq!(summary.outcome, Outcome::Passed);

    Ok(())
}

#[test]
fn inventory_totals_reconcile_with_manual_walk() -> Result<()> {
    let logs_root = tempdir()?;
    write_flash_bundle(&logs_root.path().join("unit_a"), &ROLES, 0)?;
    fs::write(logs_root.path().join("standalone.log"), "abcdef")?;
    fs::create_dir_all(logs_root.path().join(".state"))?;
    fs::write(logs_root.path().join(".state").join("lock"), "x")?;

    let inv = build_inventory(logs_root.path())?;

    fn walk(dir: &Path, total: &mut u64, count: &mut usize) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                walk(&entry.path(), total, count)?;
            } else {
                *total += entry.metadata()?.len();
                *count += 1;
            }
        }
        Ok(())
    }
    let mut manual_total = 0u64;
    let mut manual_count = 0usize;
    walk(logs_root.path(), &mut manual_total, &mut manual_count)?;

    assert_eq!(inv.tree_totals.total_bytes, manual_total);
    assert_eq!(inv.tree_totals.total_files, manual_count);

    Ok(())
}

// --- Phase 2: run/standalone expiry -----------------------------------

#[test]
fn without_runs_flag_ignores_whole_unit_and_standalone_expiry() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("old_passed_unit");
    fs::create_dir_all(&unit)?;
    write_report(&unit, "passed", 31)?;
    write_flash_bundle(&unit, &["app.bin"], 31)?;
    write_standalone(logs_root.path(), "old_standalone.log", 31)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert!(plan.expired_units.is_empty());
    assert!(plan.expired_standalone.is_empty());
    // Payload thinning still applies unchanged: the app.bin is 31 days old.
    assert_eq!(plan.candidates.len(), 1);

    Ok(())
}

#[test]
fn runs_expires_passed_unit_past_thirty_days_and_skips_its_payloads() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("wifi_regression_gate_old");
    write_flash_bundle(&unit, &ROLES, 31)?;
    write_report(&unit, "passed", 31)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        SystemTime::now(),
    )?;
    assert_eq!(plan.expired_units.len(), 1);
    assert_eq!(plan.expired_units[0].outcome, Outcome::Passed);
    assert!(plan.candidates.is_empty());

    Ok(())
}

#[test]
fn runs_keeps_passed_unit_before_thirty_days_and_still_thins_its_payloads() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("wifi_regression_gate_recent");
    write_flash_bundle(&unit, &ROLES, 10)?;
    write_report(&unit, "passed", 10)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        SystemTime::now(),
    )?;
    assert!(plan.expired_units.is_empty());
    // Unit itself survives, but its payloads are still 10 days old (>7d).
    assert_eq!(plan.candidates.len(), 4);

    Ok(())
}

#[test]
fn newer_evidence_prevents_expiry_from_a_stale_report() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("continued_after_report");
    write_flash_bundle(&unit, &ROLES, 1)?;
    write_report(&unit, "passed", 31)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        SystemTime::now(),
    )?;
    assert!(plan.expired_units.is_empty());

    Ok(())
}

#[test]
fn run_and_standalone_expiry_boundaries_use_fixed_now() -> Result<()> {
    let logs_root = tempdir()?;
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000);
    let day = Duration::from_secs(24 * 3600);

    for (name, outcome, age) in [
        ("passed_exact", Some("passed"), 30 * day),
        (
            "passed_before",
            Some("passed"),
            30 * day - Duration::from_secs(1),
        ),
        ("failed_exact", Some("failed"), 90 * day),
        (
            "failed_before",
            Some("failed"),
            90 * day - Duration::from_secs(1),
        ),
        ("inconclusive_exact", None, 90 * day),
        (
            "inconclusive_before",
            None,
            90 * day - Duration::from_secs(1),
        ),
    ] {
        let unit = logs_root.path().join(name);
        let modified = now - age;
        write_flash_bundle_at(&unit, &["app.bin"], modified)?;
        if let Some(outcome) = outcome {
            write_report_at(&unit, outcome, modified)?;
        }
    }

    let standalone_exact = write_standalone(logs_root.path(), "standalone_exact.log", 0)?;
    touch_at(&standalone_exact, now - 30 * day)?;
    let standalone_before = write_standalone(logs_root.path(), "standalone_before.log", 0)?;
    touch_at(
        &standalone_before,
        now - (30 * day - Duration::from_secs(1)),
    )?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        now,
    )?;
    let names: Vec<&str> = plan
        .expired_units
        .iter()
        .map(|unit| unit.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["failed_exact", "inconclusive_exact", "passed_exact"]
    );
    let standalone: Vec<&str> = plan
        .expired_standalone
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(standalone, vec!["standalone_exact.log"]);

    Ok(())
}

#[test]
fn runs_expires_inconclusive_unit_past_ninety_days_not_at_forty() -> Result<()> {
    let logs_root = tempdir()?;
    let young_failure_window = logs_root.path().join("boot_scan_40d");
    write_flash_bundle(&young_failure_window, &ROLES, 40)?;
    let expired = logs_root.path().join("boot_scan_91d");
    write_flash_bundle(&expired, &ROLES, 91)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        SystemTime::now(),
    )?;
    let expired_names: Vec<&str> = plan.expired_units.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(expired_names, vec!["boot_scan_91d"]);
    assert_eq!(plan.expired_units[0].outcome, Outcome::Inconclusive);
    // The 40-day unit is not whole-unit-expired (needs 90d), so its
    // payloads still surface as ordinary thinning candidates (>7d).
    assert_eq!(plan.candidates.len(), 4);

    Ok(())
}

#[test]
fn runs_ignore_age_expires_a_brand_new_unit() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("wifi_regression_gate_fresh");
    write_flash_bundle(&unit, &ROLES, 0)?;
    write_report(&unit, "passed", 0)?;

    let default_plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        SystemTime::now(),
    )?;
    assert!(default_plan.expired_units.is_empty());

    let ignore_age_plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: true,
            runs: true,
        },
        SystemTime::now(),
    )?;
    assert_eq!(ignore_age_plan.expired_units.len(), 1);
    assert!(ignore_age_plan.candidates.is_empty());

    Ok(())
}

#[test]
fn runs_retained_unit_with_evidence_scope_survives_but_still_thins_payloads() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("old_retained_evidence");
    write_flash_bundle(&unit, &ROLES, 100)?;
    write_retain_json(&unit.join(".retain.json"), "evidence", "2099-01-01")?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        SystemTime::now(),
    )?;
    assert!(plan.expired_units.is_empty());
    assert_eq!(plan.retained_unit_count, 1);
    // `evidence` protects the unit from whole-unit expiry but not its
    // payloads from ordinary thinning.
    assert_eq!(plan.candidates.len(), 4);

    Ok(())
}

#[test]
fn runs_retained_unit_with_debug_scope_blocks_both_unit_and_payload_removal() -> Result<()> {
    let logs_root = tempdir()?;
    let unit = logs_root.path().join("old_retained_debug");
    write_flash_bundle(&unit, &ROLES, 100)?;
    write_retain_json(&unit.join(".retain.json"), "debug", "2099-01-01")?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        SystemTime::now(),
    )?;
    assert!(plan.expired_units.is_empty());
    assert_eq!(plan.retained_unit_count, 1);
    assert!(plan.candidates.is_empty());
    assert_eq!(plan.retained_payload_count, 4);

    Ok(())
}

#[test]
fn runs_expires_standalone_logs_past_thirty_days_and_honors_retention() -> Result<()> {
    let logs_root = tempdir()?;
    write_standalone(logs_root.path(), "recent.log", 10)?;
    write_standalone(logs_root.path(), "expired.log", 31)?;
    let retained = write_standalone(logs_root.path(), "retained.log", 31)?;
    let retain_path = PathBuf::from(format!("{}.retain.json", retained.display()));
    write_retain_json(&retain_path, "evidence", "2099-01-01")?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        SystemTime::now(),
    )?;
    let expired_names: Vec<&str> = plan
        .expired_standalone
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(expired_names, vec!["expired.log"]);
    assert_eq!(plan.retained_standalone_count, 1);

    Ok(())
}

#[test]
fn runs_ignore_age_includes_recent_standalone() -> Result<()> {
    let logs_root = tempdir()?;
    write_standalone(logs_root.path(), "recent.log", 0)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: true,
            runs: true,
        },
        SystemTime::now(),
    )?;
    assert_eq!(plan.expired_standalone.len(), 1);
    assert_eq!(plan.expired_standalone[0].name, "recent.log");

    Ok(())
}

#[test]
fn runs_apply_removes_expired_units_and_standalone_leaves_retained_content() -> Result<()> {
    let logs_root = tempdir()?;

    let expiring_unit = logs_root.path().join("wifi_regression_gate_expiring");
    write_flash_bundle(&expiring_unit, &["app.bin"], 31)?;
    write_report(&expiring_unit, "passed", 31)?;

    let retained_unit = logs_root.path().join("old_retained_evidence");
    write_flash_bundle(&retained_unit, &ROLES, 100)?;
    write_retain_json(
        &retained_unit.join(".retain.json"),
        "evidence",
        "2099-01-01",
    )?;

    let young_unit = logs_root.path().join("young_unit");
    write_flash_bundle(&young_unit, &ROLES, 1)?;

    let expiring_standalone = write_standalone(logs_root.path(), "expiring.log", 31)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        SystemTime::now(),
    )?;
    let (report, report_path) = apply_prune_plan(logs_root.path(), &plan)?;

    assert!(!expiring_unit.exists());
    assert!(!expiring_standalone.exists());
    assert!(retained_unit.exists());
    // evidence scope protects the unit but not its payloads.
    for role in ROLES {
        assert!(!retained_unit.join(role).exists());
    }
    assert!(young_unit.join("app.bin").exists());

    assert_eq!(report.removed_units_count, 1);
    assert!(report.units_reclaimed_bytes > 0);
    assert_eq!(report.removed_standalone_count, 1);
    assert!(report.standalone_reclaimed_bytes > 0);
    assert_eq!(report.removed_count, ROLES.len());

    let report_json: serde_json::Value = serde_json::from_slice(&fs::read(&report_path)?)?;
    assert_eq!(report_json["status"], "completed");
    assert_eq!(
        report_json["removed_units"][0]["unit"],
        "wifi_regression_gate_expiring"
    );
    assert_eq!(report_json["removed_units"][0]["outcome"], "passed");
    assert_eq!(
        report_json["removed_units"][0]["path"],
        expiring_unit.display().to_string()
    );
    assert!(
        report_json["removed_units"][0]["age_days"]
            .as_f64()
            .unwrap()
            >= 30.0
    );
    assert!(
        report_json["removed_units"][0]["total_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(report_json["removed_standalone"][0]["name"], "expiring.log");
    assert_eq!(
        report_json["removed_standalone"][0]["path"],
        expiring_standalone.display().to_string()
    );
    assert!(
        report_json["removed_standalone"][0]["age_days"]
            .as_f64()
            .unwrap()
            >= 30.0
    );
    assert!(
        report_json["removed_standalone"][0]["size_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );

    let second_plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        SystemTime::now(),
    )?;
    let (second_report, _) = apply_prune_plan(logs_root.path(), &second_plan)?;
    assert_eq!(second_report.removed_units_count, 0);
    assert_eq!(second_report.units_reclaimed_bytes, 0);
    assert_eq!(second_report.removed_standalone_count, 0);
    assert_eq!(second_report.removed_count, 0);

    Ok(())
}

#[test]
fn runs_expires_direct_json_prune_reports_at_ninety_days() -> Result<()> {
    let logs_root = tempdir()?;
    let reports_root = logs_root.path().join(".prune-reports");
    fs::create_dir_all(&reports_root)?;
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000);
    let ninety_days = Duration::from_secs(90 * 24 * 3600);

    let exact = reports_root.join("exact.json");
    fs::write(&exact, "{}")?;
    touch_at(&exact, now - ninety_days)?;
    let before = reports_root.join("before.json");
    fs::write(&before, "{}")?;
    touch_at(&before, now - (ninety_days - Duration::from_secs(1)))?;
    let ignored = reports_root.join("old.txt");
    fs::write(&ignored, "not a report")?;
    touch_at(&ignored, now - ninety_days)?;

    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: true,
        },
        now,
    )?;
    assert_eq!(plan.expired_reports.len(), 1);
    assert_eq!(plan.expired_reports[0].name, "exact.json");

    let ignore_age_plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: true,
            runs: true,
        },
        now,
    )?;
    let ignore_age_names: Vec<&str> = ignore_age_plan
        .expired_reports
        .iter()
        .map(|report| report.name.as_str())
        .collect();
    assert_eq!(ignore_age_names, vec!["before.json", "exact.json"]);

    let (_, report_path) = apply_prune_plan(logs_root.path(), &plan)?;
    assert!(!exact.exists());
    assert!(before.exists());
    assert!(ignored.exists());
    assert!(report_path.exists());
    let report_json: serde_json::Value = serde_json::from_slice(&fs::read(report_path)?)?;
    assert_eq!(report_json["removed_prune_reports_count"], 1);
    assert_eq!(
        report_json["removed_prune_reports"][0]["name"],
        "exact.json"
    );
    assert_eq!(
        report_json["removed_prune_reports"][0]["path"],
        exact.display().to_string()
    );
    assert!(
        report_json["removed_prune_reports"][0]["age_days"]
            .as_f64()
            .unwrap()
            >= 90.0
    );
    assert_eq!(report_json["removed_prune_reports"][0]["size_bytes"], 2);
    assert_eq!(report_json["status"], "completed");

    Ok(())
}

#[test]
fn failed_apply_audits_successful_removals() -> Result<()> {
    let logs_root = tempdir()?;
    write_flash_bundle(&logs_root.path().join("expired"), &ROLES, 10)?;
    let plan = build_prune_plan(
        logs_root.path(),
        &PruneOptions {
            ignore_age: false,
            runs: false,
        },
        SystemTime::now(),
    )?;
    assert!(plan.candidates.len() > 1);
    let first = plan.candidates[0].path.clone();
    let failing = plan.candidates[1].path.clone();
    fs::remove_file(&failing)?;

    let error = apply_prune_plan(logs_root.path(), &plan).unwrap_err();
    assert!(error.to_string().contains("remove payload"));
    assert!(!first.exists());

    let reports: Vec<PathBuf> = fs::read_dir(logs_root.path().join(".prune-reports"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    assert_eq!(reports.len(), 1);
    let report_json: serde_json::Value = serde_json::from_slice(&fs::read(&reports[0])?)?;
    assert_eq!(report_json["status"], "failed");
    assert_eq!(report_json["removed_count"], 1);
    assert_eq!(
        report_json["removed"][0]["path"],
        first.display().to_string()
    );
    assert!(report_json["error"]
        .as_str()
        .unwrap()
        .contains(&failing.display().to_string()));

    Ok(())
}
