mod classify;
mod context;
mod probes;
mod runtime_core;
mod runtime_steps;
mod utils;

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use chrono::Local;
use serde_json::{json, Value};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow},
    serial_console::SerialConsole,
};

#[derive(Clone, Debug)]
pub struct TroubleshootOptions {
    pub build_mode: String,
    pub output_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct TroubleshootConfig {
    flash_first: bool,
    flash_retries: u32,
    probe_retries: u32,
    probe_delay_ms: u64,
    probe_timeout_ms: u64,
    soak_cycles: u32,
}

struct TroubleshootRuntime<'a> {
    logger: &'a mut Logger,
    config: TroubleshootConfig,
    build_mode: String,
    port: String,
    baud: u32,
    uart_log_path: PathBuf,
    soak_log_dir: PathBuf,
    console: Option<SerialConsole>,
    result: String,
    failure_stage: String,
    failure_class: String,
    failure_detail: String,
    flash_ok: bool,
    probe_ok: bool,
    soak_ok: bool,
}

pub fn run_troubleshoot(logger: &mut Logger, opts: TroubleshootOptions) -> Result<()> {
    let flash_first = env_utils::parse_env_bool01("HOSTCTL_TROUBLESHOOT_FLASH_FIRST", true)?;
    let flash_retries = env_utils::parse_env_u32("HOSTCTL_TROUBLESHOOT_FLASH_RETRIES", 2)?.max(1);
    let probe_retries = env_utils::parse_env_u32("HOSTCTL_TROUBLESHOOT_PROBE_RETRIES", 6)?.max(1);
    let probe_delay_ms = env_utils::parse_env_u64("HOSTCTL_TROUBLESHOOT_PROBE_DELAY_MS", 700)?;
    let probe_timeout_ms = env_utils::parse_env_u64("HOSTCTL_TROUBLESHOOT_PROBE_TIMEOUT_MS", 4000)?;
    let soak_cycles = env_utils::parse_env_u32("HOSTCTL_TROUBLESHOOT_SOAK_CYCLES", 4)?.max(1);

    let config = TroubleshootConfig {
        flash_first,
        flash_retries,
        probe_retries,
        probe_delay_ms,
        probe_timeout_ms,
        soak_cycles,
    };

    let ts = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let uart_log_path = opts
        .output_path
        .unwrap_or_else(|| PathBuf::from(format!("logs/troubleshoot_{ts}.log")));
    ensure_parent_dir(&uart_log_path)?;

    let soak_log_dir = PathBuf::from(format!("logs/troubleshoot_soak_{ts}"));
    std::fs::create_dir_all(&soak_log_dir)?;

    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115200)?;

    let workflow_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/troubleshoot.sw.yaml");
    let workflow = load_workflow(&workflow_path)?;

    let mut runtime = TroubleshootRuntime::new(
        logger,
        config,
        opts.build_mode,
        port,
        baud,
        uart_log_path,
        soak_log_dir,
    );

    let context = execute_workflow(&workflow, &mut runtime, &json!({}))?;

    if context
        .get("result")
        .and_then(Value::as_str)
        .is_some_and(|result| result == "passed")
    {
        return Ok(());
    }

    Err(anyhow!(
        "troubleshoot failed: stage={} class={}",
        runtime.failure_stage,
        runtime.failure_class
    ))
}
