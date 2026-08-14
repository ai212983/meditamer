//! `hostctl artifacts inventory` / `hostctl artifacts prune` --
//! `docs/archive/host-tooling/log-and-artifact-pruning.md`: read-only inventory of
//! `logs/`, thinning of recognized flash-capture payloads (Phase 1), and,
//! with `--runs`, whole-run/standalone-log expiry (Phase 2).

mod apply;
mod expiry;
mod inventory;
mod model;
mod outcome;
mod prune;
mod report;
mod retention;
mod scan;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;

use crate::logging::Logger;
use crate::workflows::common::repo_root;

pub fn run_artifacts_inventory(logger: &mut Logger) -> Result<()> {
    let logs_root = artifacts_logs_root();
    let inv = inventory::build_inventory(&logs_root)?;
    inventory::print_inventory(logger, &inv);
    Ok(())
}

pub struct ArtifactsPruneOptions {
    pub apply: bool,
    pub ignore_age: bool,
    pub runs: bool,
}

pub fn run_artifacts_prune(logger: &mut Logger, opts: ArtifactsPruneOptions) -> Result<()> {
    let logs_root = artifacts_logs_root();
    let plan_opts = prune::PruneOptions {
        ignore_age: opts.ignore_age,
        runs: opts.runs,
    };
    let plan = prune::build_prune_plan(&logs_root, &plan_opts, SystemTime::now())?;
    prune::print_prune_plan(logger, &plan);

    if opts.apply {
        let (report, report_path) = apply::apply_prune_plan(&logs_root, &plan)?;
        apply::print_apply_result(logger, &report, &report_path);
    }

    Ok(())
}

fn artifacts_logs_root() -> PathBuf {
    repo_root().join("logs")
}
