fn capture_boot_window(
    port: &str,
    baud: u32,
    duration: Duration,
    output_path: &Path,
) -> Result<Vec<u8>> {
    let mut console = SerialConsole::open(port, baud, None)?;
    console.clear_input_buffer().ok();
    console.pulse_en_reset(120, 20)?;
    let bytes = console.capture_raw_for(duration)?;
    fs::write(output_path, &bytes)?;
    Ok(bytes)
}

fn capture_stream_window(
    port: &str,
    baud: u32,
    duration: Duration,
    output_path: &Path,
) -> Result<Vec<u8>> {
    let mut console = SerialConsole::open(port, baud, None)?;
    console.clear_input_buffer().ok();
    let bytes = console.capture_raw_for(duration)?;
    fs::write(output_path, &bytes)?;
    Ok(bytes)
}

fn run_timeset_after_flash(logger: &mut Logger, port: &str, baud: u32) -> Result<()> {
    let original_port = env::var("HOSTCTL_PORT").ok();
    let original_baud = env::var("HOSTCTL_BAUD").ok();
    env::set_var("HOSTCTL_PORT", port);
    env::set_var("HOSTCTL_BAUD", baud.to_string());
    let result = run_timeset(
        logger,
        TimeSetOptions {
            epoch: None,
            tz_offset_minutes: None,
        },
    );
    restore_env_var("HOSTCTL_PORT", original_port);
    restore_env_var("HOSTCTL_BAUD", original_baud);
    result
}

fn restore_env_var(name: &str, value: Option<String>) {
    if let Some(value) = value {
        env::set_var(name, value);
    } else {
        env::remove_var(name);
    }
}

fn write_summary(
    outputs: &OutputPaths,
    port: &str,
    baud: u32,
    flash_baud: u32,
    result: &FlashResult,
    capture_mode: CaptureMode,
    capture_bytes: usize,
) -> Result<()> {
    let mut file = File::create(&outputs.summary)?;
    writeln!(file, "port={port}")?;
    writeln!(file, "baud={baud}")?;
    writeln!(file, "flash_baud={flash_baud}")?;
    writeln!(file, "strategy={:?}", result.strategy)?;
    writeln!(file, "capture_mode={:?}", capture_mode)?;
    writeln!(file, "capture_bytes={capture_bytes}")?;
    writeln!(file, "image_path={}", result.image_path.display())?;
    writeln!(file, "fallback_used={}", result.fallback_used)?;
    writeln!(file, "python_bin={}", display_opt_path(result.python_bin.as_ref()))?;
    writeln!(file, "idf_root={}", display_opt_path(result.idf_root.as_ref()))?;
    writeln!(file, "idf_py_bin={}", display_opt_path(result.idf_py_bin.as_ref()))?;
    writeln!(file, "reset_mode=en-only")?;
    Ok(())
}

fn display_opt_path(path: Option<&PathBuf>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "n/a".to_string())
}
