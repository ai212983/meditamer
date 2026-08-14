//! Phase 2: whole-run and standalone expiry data shapes and the
//! outcome-based age thresholds from the plan's retention policy table.
//! Candidate selection happens inline in `prune::build_prune_plan` (one
//! scan pass per unit); this module holds the shared shapes and the
//! threshold rule so selection and reporting agree on both.

use std::path::PathBuf;
use std::time::Duration;

use super::model::Outcome;

pub const SUCCESSFUL_UNIT_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
pub const FAILED_OR_INCONCLUSIVE_UNIT_MAX_AGE: Duration = Duration::from_secs(90 * 24 * 3600);
pub const STANDALONE_LOG_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
pub const PRUNE_REPORT_MAX_AGE: Duration = Duration::from_secs(90 * 24 * 3600);

/// The run-unit expiry threshold for `outcome`: 30 days for a successful
/// run, 90 days for a failed or inconclusive one.
pub fn unit_max_age(outcome: Outcome) -> Duration {
    match outcome {
        Outcome::Passed => SUCCESSFUL_UNIT_MAX_AGE,
        Outcome::Failed | Outcome::Inconclusive => FAILED_OR_INCONCLUSIVE_UNIT_MAX_AGE,
    }
}

/// A whole run unit selected for removal: age/outcome expired, unretained.
pub struct ExpiredUnit {
    pub name: String,
    pub path: PathBuf,
    pub outcome: Outcome,
    pub age_days: f64,
    pub total_bytes: u64,
}

/// A standalone log selected for removal: age expired, unretained.
pub struct ExpiredStandalone {
    pub name: String,
    pub path: PathBuf,
    pub age_days: f64,
    pub size_bytes: u64,
}

/// A direct JSON file in `.prune-reports/` selected for expiry.
pub struct ExpiredPruneReport {
    pub name: String,
    pub path: PathBuf,
    pub age_days: f64,
    pub size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passed_units_expire_at_thirty_days() {
        assert_eq!(unit_max_age(Outcome::Passed), SUCCESSFUL_UNIT_MAX_AGE);
    }

    #[test]
    fn failed_and_inconclusive_units_expire_at_ninety_days() {
        assert_eq!(
            unit_max_age(Outcome::Failed),
            FAILED_OR_INCONCLUSIVE_UNIT_MAX_AGE
        );
        assert_eq!(
            unit_max_age(Outcome::Inconclusive),
            FAILED_OR_INCONCLUSIVE_UNIT_MAX_AGE
        );
    }
}
