use anyhow::{anyhow, Result};
use serde_json::Value;

use super::report::{all_required_pass, assemble_report, caught_message, checkpoint_satisfied};
use super::AssetResidencyRuntime;
use crate::logging::ensure_parent_dir;

impl AssetResidencyRuntime<'_> {
    pub(super) fn dispatch_report_actions(
        &mut self,
        action: &str,
        _args: &Value,
    ) -> Option<Result<()>> {
        match action {
            "print_summary" => Some(self.print_summary()),
            "fail_evidence" => Some(self.fail_evidence()),
            _ => None,
        }
    }

    pub(super) fn finalize_impl(&mut self, context: &Value) -> Result<bool> {
        let caught_error = caught_message(context);
        let run_passed = caught_error.is_none()
            && all_required_pass(&self.checkpoints, &self.required_checkpoints);
        let report = assemble_report(
            &self.feature_label,
            &self.build_label,
            &self.started_at,
            &self.required_checkpoints,
            &self.checkpoints,
            &self.state_lines,
            caught_error,
        );
        ensure_parent_dir(&self.report_path)?;
        std::fs::write(&self.report_path, serde_json::to_vec_pretty(&report)?)?;
        self.report = Some(report);
        Ok(run_passed)
    }

    fn print_summary(&mut self) -> Result<()> {
        let report = self
            .report
            .as_ref()
            .ok_or_else(|| anyhow!("missing asset residency report"))?;
        let passed = report
            .checkpoints
            .iter()
            .filter(|entry| checkpoint_satisfied(&entry.name, &entry.status))
            .count();
        let inferred = report
            .checkpoints
            .iter()
            .filter(|entry| entry.status == "inferred")
            .count();
        self.logger.info(format!(
            "Asset residency {} satisfied: {}/{} checkpoints ({} inferred) report={}",
            if self.active_cycle.is_some() {
                "reentry"
            } else {
                "baseline"
            },
            passed,
            report.checkpoints.len(),
            inferred,
            self.report_path.display()
        ));
        Ok(())
    }

    fn fail_evidence(&mut self) -> Result<()> {
        let report = self
            .report
            .as_ref()
            .ok_or_else(|| anyhow!("missing asset residency report"))?;
        let gaps = report
            .checkpoints
            .iter()
            .filter(|entry| !checkpoint_satisfied(&entry.name, &entry.status))
            .map(|entry| format!("{}={}", entry.name, entry.status))
            .collect::<Vec<_>>()
            .join(", ");
        Err(anyhow!(
            "asset residency {} failed ({}): report={}",
            if self.active_cycle.is_some() {
                "reentry"
            } else {
                "baseline"
            },
            if gaps.is_empty() {
                report
                    .caught_error
                    .clone()
                    .unwrap_or_else(|| "measurement error".to_string())
            } else {
                gaps
            },
            self.report_path.display()
        ))
    }
}
