pub fn run_runtime_modes_smoke(logger: &mut Logger, opts: RuntimeModesSmokeOptions) -> Result<()> {
    let settle_ms = env_utils::parse_env_u64("HOSTCTL_MODE_SMOKE_SETTLE_MS", 0)?;
    let post_upload_status_repeats =
        env_utils::parse_env_u32("HOSTCTL_MODE_SMOKE_POST_UPLOAD_STATUS_REPEATS", 3)?;
    let post_upload_timeset_repeats =
        env_utils::parse_env_u32("HOSTCTL_MODE_SMOKE_POST_UPLOAD_TIMESET_REPEATS", 2)?;

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
        post_upload_timeset_repeats,
    );

    let _ = execute_workflow(&workflow, &mut runtime, &json!({}))?;

    logger.info(format!(
        "Runtime mode smoke passed. Log: {}",
        output_path.display()
    ));
    Ok(())
}
