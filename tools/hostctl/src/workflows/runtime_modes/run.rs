use std::path::PathBuf;

use anyhow::{anyhow, Result};
use chrono::Local;
use serde_json::json;

use super::runtime::{open_console, RuntimeModesScenarioRuntime, RuntimeModesSmokeOptions};
use crate::{
    logging::Logger,
    scenarios::{execute_workflow, load_workflow},
};

pub fn run_runtime_modes_smoke(logger: &mut Logger, opts: RuntimeModesSmokeOptions) -> Result<()> {
    if !matches!(opts.suite.as_str(), "full" | "no-storage") {
        return Err(anyhow!(
            "invalid runtime-mode suite `{}` (use full|no-storage)",
            opts.suite
        ));
    }
    // Smoke-test internals; formerly env-tunable (hostctl-env-audit.md cat 3).
    let settle_ms = 0u64;
    let post_upload_status_repeats = 3u32;
    let post_upload_ping_repeats = 2u32;

    let output_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(format!(
            "logs/runtime_modes_smoke_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ))
    });

    logger.info(format!(
        "Starting serial capture: {}",
        output_path.display()
    ));
    let console = open_console(&output_path)?;

    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/runtime-modes-smoke.sw.yaml"),
    )?;
    let mut runtime = RuntimeModesScenarioRuntime::new(
        logger,
        console,
        settle_ms,
        post_upload_status_repeats,
        post_upload_ping_repeats,
    );

    let _ = execute_workflow(&workflow, &mut runtime, &json!({ "suite": opts.suite }))?;

    logger.info(format!(
        "Runtime mode smoke passed. Log: {}",
        output_path.display()
    ));
    Ok(())
}
