//! Scenario entry points and serial-session setup for asset residency.

use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use chrono::Local;
use serde_json::json;

use super::{
    reentry_required_checkpoints, report_path_for, AssetResidencyBaselineOptions,
    AssetResidencyRuntime, REQUIRED_CHECKPOINTS,
};
use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow},
    serial_console::SerialConsole,
    workflows::wifi::common::acquire_port_lock,
};

pub fn run_asset_residency_baseline(
    logger: &mut Logger,
    opts: AssetResidencyBaselineOptions,
) -> Result<()> {
    run_residency_scenario(
        logger,
        opts,
        "scenarios/asset-residency-baseline.sw.yaml",
        "asset_residency_baseline",
        false,
        0,
    )
}

/// Repeated same-boot Mountain/Clock reentry with upload-mode allocation
/// churn. The firmware image is already flashed; the scenario never resets
/// or retries an ambiguous UI command.
pub fn run_asset_residency_reentry(
    logger: &mut Logger,
    opts: AssetResidencyBaselineOptions,
    cycles: u8,
) -> Result<()> {
    if !(2..=10).contains(&cycles) {
        return Err(anyhow!("reentry cycles must be in 2..=10"));
    }
    run_residency_scenario(
        logger,
        opts,
        "scenarios/asset-residency-reentry.sw.yaml",
        "asset_residency_reentry",
        false,
        cycles,
    )
}

/// Mountain-off/upload-on-clock stress. Firmware admission intentionally
/// blocks Mountain requests while upload is enabled, so the scenario
/// switches upload on only after Mountain composition and then holds the
/// clock across the LACT-low rollover with serial/TCP liveness checks.
pub fn run_asset_residency_stress(
    logger: &mut Logger,
    opts: AssetResidencyBaselineOptions,
) -> Result<()> {
    run_residency_scenario(
        logger,
        opts,
        "scenarios/asset-residency-baseline.sw.yaml",
        "asset_residency_stress",
        true,
        0,
    )
}

/// Ambient-soak `METRICS` headroom watch for the stack-overflow hunt: reset
/// already-flashed firmware, wait for Ambient adoption, then sample both
/// stack minima on an even grid. The mountain/checkpoint gate is expected
/// to fail (those checkpoints never apply here); the headroom trend in the
/// report's state lines is the evidence.
pub fn run_stack_headroom_watch(
    logger: &mut Logger,
    opts: AssetResidencyBaselineOptions,
) -> Result<()> {
    run_residency_scenario(
        logger,
        opts,
        "scenarios/stack-headroom-watch.sw.yaml",
        "stack_headroom_watch",
        false,
        0,
    )
}

/// Bisection variant of the headroom watch: the scenario switches the
/// upload service off after Ambient adoption, gating the HTTP listener
/// accept loop while Wi-Fi stays associated. Same grid, same abort and
/// side-channel behavior; the log prefix keeps its artifacts apart.
pub fn run_stack_headroom_watch_upload_off(
    logger: &mut Logger,
    opts: AssetResidencyBaselineOptions,
) -> Result<()> {
    run_residency_scenario(
        logger,
        opts,
        "scenarios/stack-headroom-watch-upload-off.sw.yaml",
        "stack_headroom_watch_upload_off",
        false,
        0,
    )
}

/// Control variant: the scenario switches the upload service explicitly on
/// after Ambient adoption, overriding any persisted off state from earlier
/// runs, so an upload-on control always tests what it claims to test.
pub fn run_stack_headroom_watch_upload_on(
    logger: &mut Logger,
    opts: AssetResidencyBaselineOptions,
) -> Result<()> {
    run_residency_scenario(
        logger,
        opts,
        "scenarios/stack-headroom-watch-upload-on.sw.yaml",
        "stack_headroom_watch_upload_on",
        false,
        0,
    )
}

fn run_residency_scenario(
    logger: &mut Logger,
    opts: AssetResidencyBaselineOptions,
    scenario_file: &str,
    log_prefix: &str,
    stress: bool,
    reentry_cycles: u8,
) -> Result<()> {
    if opts.feature_label.is_empty() || opts.build_label.is_empty() {
        return Err(anyhow!("feature-label and build-label must be non-empty"));
    }
    if opts.marker_timeout_secs == 0 {
        return Err(anyhow!("marker-timeout-secs must be nonzero"));
    }
    if opts.hold_samples > 7 {
        return Err(anyhow!("hold-samples must be between 0 and 7"));
    }
    let log_path = opts.output_path.clone().unwrap_or_else(|| {
        PathBuf::from(format!(
            "logs/{log_prefix}_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ))
    });
    let report_path = report_path_for(&log_path);
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115_200)?;
    ensure_parent_dir(&log_path)?;
    let _port_lock = acquire_port_lock(&port)?;
    let workflow = load_workflow(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(scenario_file))?;
    let console = if reentry_cycles > 0 {
        // Re-entry qualification starts from the current boot; attaching the
        // console must not toggle modem lines on the programming adapter.
        SerialConsole::open_passive(&port, baud, Some(&log_path))?
    } else {
        SerialConsole::open(&port, baud, Some(&log_path))?
    };
    let started_at = chrono::Utc::now().to_rfc3339();
    let run_start = Instant::now();
    let run_mark = console.mark();
    let mut runtime = AssetResidencyRuntime {
        logger,
        console,
        consecutive_misses: 0,
        last_net: None,
        feature_label: opts.feature_label.clone(),
        build_label: opts.build_label.clone(),
        marker_timeout: Duration::from_secs(opts.marker_timeout_secs),
        started_at,
        run_start,
        run_mark,
        phase_name: "boot".to_string(),
        phase_mark: run_mark,
        report_path,
        required_checkpoints: if reentry_cycles == 0 {
            REQUIRED_CHECKPOINTS
                .iter()
                .map(|name| (*name).to_string())
                .collect()
        } else {
            reentry_required_checkpoints(reentry_cycles)
        },
        active_cycle: None,
        checkpoints: HashMap::new(),
        state_lines: Vec::new(),
        report: None,
    };
    let _ = execute_workflow(
        &workflow,
        &mut runtime,
        &json!({
            "feature_label": opts.feature_label,
            "build_label": opts.build_label,
            "stress": stress,
            "stress_hold_samples": opts.hold_samples,
            "reentry_cycles": reentry_cycles,
        }),
    )?;
    Ok(())
}
