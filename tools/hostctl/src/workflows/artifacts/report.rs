//! Reads the sole structured report gate currently producing a run outcome:
//! `report.json` written by `scripts/tests/hw/test_wifi_regression_gate.sh`
//! (`final_status` / `finished_at`). Any run unit without this file -- every
//! flash-capture bundle, troubleshoot soak, or other producer today -- has
//! no recognized report and is inconclusive per the plan's run-age rule.
//! Adding other producers' report shapes is out of this phase's scope.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};
use chrono::DateTime;
use serde::Deserialize;

use super::model::Outcome;

#[derive(Debug, Deserialize)]
struct RecognizedReport {
    final_status: String,
    finished_at: String,
}

pub struct ReportOutcome {
    pub outcome: Outcome,
    pub completed_at: SystemTime,
}

pub fn read_recognized_report(unit_dir: &Path) -> Result<Option<ReportOutcome>> {
    let path = unit_dir.join("report.json");
    if !path.is_file() {
        return Ok(None);
    }

    let text =
        fs::read_to_string(&path).with_context(|| format!("read report {}", path.display()))?;
    let report: RecognizedReport =
        serde_json::from_str(&text).with_context(|| format!("parse report {}", path.display()))?;
    let completed_at: SystemTime = DateTime::parse_from_rfc3339(&report.finished_at)
        .with_context(|| format!("report {} has an invalid finished_at", path.display()))?
        .into();

    let outcome = if report.final_status == "passed" {
        Outcome::Passed
    } else {
        Outcome::Failed
    };

    Ok(Some(ReportOutcome {
        outcome,
        completed_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_report_is_none() -> Result<()> {
        let dir = tempdir()?;
        assert!(read_recognized_report(dir.path())?.is_none());
        Ok(())
    }

    #[test]
    fn passed_report_parses() -> Result<()> {
        let dir = tempdir()?;
        fs::write(
            dir.path().join("report.json"),
            r#"{"final_status":"passed","finished_at":"2026-08-13T15:57:42Z"}"#,
        )?;
        let report = read_recognized_report(dir.path())?.expect("report present");
        assert_eq!(report.outcome, Outcome::Passed);
        Ok(())
    }

    #[test]
    fn failed_report_parses() -> Result<()> {
        let dir = tempdir()?;
        fs::write(
            dir.path().join("report.json"),
            r#"{"final_status":"failed","finished_at":"2026-08-13T15:57:42Z"}"#,
        )?;
        let report = read_recognized_report(dir.path())?.expect("report present");
        assert_eq!(report.outcome, Outcome::Failed);
        Ok(())
    }

    #[test]
    fn malformed_report_errors() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join("report.json"), "not json")?;
        assert!(read_recognized_report(dir.path()).is_err());
        Ok(())
    }
}
