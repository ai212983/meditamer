use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;
use chrono::Local;
use serde_json::json;

use crate::{
    env_utils,
    logging::Logger,
    scenarios::{execute_workflow, load_workflow},
};

use super::{
    io::{force_upload_mode_off, maybe_flash_first, open_console},
    runtime::SdcardScenarioRuntime,
    types::{suite_name, SdcardHwOptions, SdcardSuite},
};

pub fn run_sdcard_hw(logger: &mut Logger, opts: SdcardHwOptions) -> Result<()> {
    maybe_flash_first(logger, &opts.build_mode)?;

    let verify_lba = env_utils::parse_env_u32("HOSTCTL_SDCARD_VERIFY_LBA", 2048)?;
    let run_tag = Local::now().format("%H%M%S").to_string();
    let base_path =
        std::env::var("HOSTCTL_SDCARD_BASE_PATH").unwrap_or_else(|_| format!("/sd{run_tag}"));
    let sdwait_timeout_ms = env_utils::parse_env_u32("HOSTCTL_SDCARD_SDWAIT_TIMEOUT_MS", 300_000)?;

    let output_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(format!(
            "logs/sdcard_hw_test_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ))
    });

    logger.info(format!(
        "Starting serial capture: {}",
        output_path.display()
    ));
    let mut console = open_console(&output_path)?;

    logger.info(format!(
        "Running SD-card command validation on {}",
        env_utils::require_port()?
    ));
    logger.info(format!("Test root path: {base_path}"));
    force_upload_mode_off(logger, &mut console)?;

    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/sdcard-hw.sw.yaml"),
    )?;

    let test_file = format!("{base_path}/io.txt");
    let test_file_renamed = format!("{base_path}/io2.txt");
    let burst_root = format!("/b{run_tag}");
    let burst_file = format!("{burst_root}/io.txt");
    let fail_root = format!("/f{run_tag}");
    let rename_root = format!("/r{run_tag}");
    let file_a = format!("{rename_root}/a.txt");
    let file_b = format!("{rename_root}/b.txt");
    let long_payload = "x".repeat(260);

    let vars = HashMap::from([
        ("run_tag".to_string(), run_tag),
        ("verify_lba".to_string(), verify_lba.to_string()),
        ("base_path".to_string(), base_path.clone()),
        ("test_file".to_string(), test_file),
        ("test_file_renamed".to_string(), test_file_renamed),
        ("burst_root".to_string(), burst_root),
        ("burst_file".to_string(), burst_file),
        ("fail_root".to_string(), fail_root),
        ("rename_root".to_string(), rename_root),
        ("file_a".to_string(), file_a),
        ("file_b".to_string(), file_b),
        ("long_payload".to_string(), long_payload),
    ]);

    let mut runtime = SdcardScenarioRuntime::new(logger, &mut console, vars, sdwait_timeout_ms);
    let _ = execute_workflow(
        &workflow,
        &mut runtime,
        &json!({
            "suite": suite_name(&opts.suite),
        }),
    )?;

    logger.info("SD-card hardware test passed");
    logger.info(format!("Log: {}", output_path.display()));

    Ok(())
}

pub fn run_sdcard_burst_regression(
    logger: &mut Logger,
    build_mode: String,
    output_path: Option<PathBuf>,
) -> Result<()> {
    run_sdcard_hw(
        logger,
        SdcardHwOptions {
            build_mode,
            output_path,
            suite: SdcardSuite::Burst,
        },
    )
}
