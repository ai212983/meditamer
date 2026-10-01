//! Reads and validates `.retain.json` retention records.
//!
//! A run unit's record lives at `<unit>/.retain.json`; a standalone file's
//! record lives adjacent as `<file>.retain.json`. A record that exists but
//! fails to parse or is missing a required field is a data error, not an
//! absence of retention -- it fails loudly rather than silently letting the
//! content it was meant to protect become prunable.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;

use super::model::RetentionRecord;

const REVIEW_AFTER_FORMAT: &str = "%Y-%m-%d";

pub fn load_run_unit_retention(unit_dir: &Path) -> Result<Option<RetentionRecord>> {
    load_retention_file(&unit_dir.join(".retain.json"))
}

pub fn load_standalone_retention(file_path: &Path) -> Result<Option<RetentionRecord>> {
    let mut retain_name = file_path
        .file_name()
        .with_context(|| format!("standalone item {} has no file name", file_path.display()))?
        .to_owned();
    retain_name.push(".retain.json");
    load_retention_file(&file_path.with_file_name(retain_name))
}

fn load_retention_file(path: &Path) -> Result<Option<RetentionRecord>> {
    if !path.is_file() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)
        .with_context(|| format!("read retention record {}", path.display()))?;
    let record: RetentionRecord = serde_json::from_str(&text)
        .with_context(|| format!("parse retention record {}", path.display()))?;

    if record.reason.trim().is_empty() {
        bail!("retention record {} has an empty reason", path.display());
    }
    if record.owner.trim().is_empty() {
        bail!("retention record {} has an empty owner", path.display());
    }
    parse_review_after(&record.review_after).with_context(|| {
        format!(
            "retention record {} has an invalid review_after",
            path.display()
        )
    })?;

    Ok(Some(record))
}

fn parse_review_after(value: &str) -> Result<NaiveDate> {
    Ok(NaiveDate::parse_from_str(value, REVIEW_AFTER_FORMAT)?)
}

/// Whether `record`'s `review_after` is due (on or before `today`).
/// `review_after` is validated at load time, so parsing here is infallible
/// in practice; a parse failure still returns `false` rather than panicking.
pub fn is_review_due(record: &RetentionRecord, today: NaiveDate) -> bool {
    parse_review_after(&record.review_after)
        .map(|review_after| review_after <= today)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_retention_file_is_none() -> Result<()> {
        let dir = tempdir()?;
        assert!(load_run_unit_retention(dir.path())?.is_none());
        Ok(())
    }

    #[test]
    fn valid_record_parses() -> Result<()> {
        let dir = tempdir()?;
        fs::write(
            dir.path().join(".retain.json"),
            r#"{"scope":"reflash","reason":"acceptance candidate","owner":"firmware","review_after":"2026-09-15"}"#,
        )?;
        let record = load_run_unit_retention(dir.path())?.expect("record present");
        assert_eq!(record.owner, "firmware");
        Ok(())
    }

    #[test]
    fn missing_review_after_is_rejected() -> Result<()> {
        let dir = tempdir()?;
        fs::write(
            dir.path().join(".retain.json"),
            r#"{"scope":"evidence","reason":"x","owner":"y"}"#,
        )?;
        assert!(load_run_unit_retention(dir.path()).is_err());
        Ok(())
    }

    #[test]
    fn empty_reason_is_rejected() -> Result<()> {
        let dir = tempdir()?;
        fs::write(
            dir.path().join(".retain.json"),
            r#"{"scope":"evidence","reason":"  ","owner":"y","review_after":"2026-09-15"}"#,
        )?;
        assert!(load_run_unit_retention(dir.path()).is_err());
        Ok(())
    }

    #[test]
    fn standalone_retention_reads_adjacent_file() -> Result<()> {
        let dir = tempdir()?;
        let target = dir.path().join("run.log");
        fs::write(&target, "log")?;
        fs::write(
            dir.path().join("run.log.retain.json"),
            r#"{"scope":"evidence","reason":"x","owner":"y","review_after":"2026-09-15"}"#,
        )?;
        assert!(load_standalone_retention(&target)?.is_some());
        Ok(())
    }

    #[test]
    fn review_due_compares_against_today() -> Result<()> {
        let record = RetentionRecord {
            scope: super::super::model::RetentionScope::Evidence,
            reason: "x".into(),
            owner: "y".into(),
            review_after: "2026-08-13".into(),
        };
        assert!(is_review_due(
            &record,
            NaiveDate::from_ymd_opt(2026, 8, 13).unwrap()
        ));
        assert!(is_review_due(
            &record,
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()
        ));
        assert!(!is_review_due(
            &record,
            NaiveDate::from_ymd_opt(2026, 8, 1).unwrap()
        ));
        Ok(())
    }
}
