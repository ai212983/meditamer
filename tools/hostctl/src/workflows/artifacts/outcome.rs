//! Shared run-unit outcome/age inference: the recognized report's
//! completion time when present, otherwise the newest evidence-file
//! modification time in the unit (the plan's "run age" rule). Used
//! identically by inventory reporting and Phase 2 run-expiry eligibility so
//! the two can never disagree about which units are old or what happened.

use std::path::Path;
use std::time::SystemTime;

use anyhow::Result;

use super::model::Outcome;
use super::report;
use super::scan::UnitScan;

pub fn infer_unit_outcome(
    unit_dir: &Path,
    unit_scan: &UnitScan,
    now: SystemTime,
) -> Result<(Outcome, SystemTime)> {
    match report::read_recognized_report(unit_dir)? {
        Some(r) => Ok((
            r.outcome,
            unit_scan
                .newest_modified
                .map_or(r.completed_at, |modified| modified.max(r.completed_at)),
        )),
        None => Ok((
            Outcome::Inconclusive,
            unit_scan.newest_modified.unwrap_or(now),
        )),
    }
}
