pub fn run_flash_capture(logger: &mut Logger, opts: FlashCaptureOptions) -> Result<()> {
    let repo_dir = repo_root();
    let (output_override, output_warning) = normalize_output_root(opts.output_path.as_deref());
    if let Some(message) = output_warning {
        logger.warn(message);
    }
    let outputs = prepare_output_paths(output_override.as_deref())?;
    let port = resolve_port(logger, opts.port.as_deref())?;
    let port_lock = acquire_port_lock(&port)?;
    ensure_port_available(&port)?;

    let baud = opts.baud.unwrap_or(env_utils::baud_from_env(115200)?);
    let flash_baud = opts
        .flash_baud
        .unwrap_or(env_utils::parse_env_u32("ESPFLASH_BAUD", 460_800)?);
    let fallback_baud = env_utils::parse_env_u32("ESPFLASH_FALLBACK_BAUD", 115_200)?;
    let flash_timeout = Duration::from_secs(env_utils::parse_env_u64("FLASH_TIMEOUT_SEC", 360)?);
    let flash_status_interval = Duration::from_secs(env_utils::parse_env_u64(
        "FLASH_STATUS_INTERVAL_SEC",
        15,
    )?);
    let flash_idle_timeout = Duration::from_secs(env_utils::parse_env_u64(
        "FLASH_IDLE_TIMEOUT_SEC",
        45,
    )?);
    let flash_progress_stall_timeout = Duration::from_secs(env_utils::parse_env_u64(
        "FLASH_PROGRESS_STALL_TIMEOUT_SEC",
        30,
    )?);
    let flash_log_drain_timeout = Duration::from_millis(env_utils::parse_env_u64(
        "FLASH_LOG_DRAIN_TIMEOUT_MS",
        1_000,
    )?);
    let boot_window = Duration::from_millis(opts.boot_window_ms.unwrap_or(
        env_utils::parse_env_u64("HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS", 8_000)?,
    ));
    let set_time_after_flash = env_utils::parse_env_bool01("FLASH_SET_TIME_AFTER_FLASH", true)?;
    let skip_update_check = env_utils::parse_env_bool01("ESPFLASH_SKIP_UPDATE_CHECK", true)?;
    let enable_fallback = env_utils::parse_env_bool01(
        "ESPFLASH_ENABLE_FALLBACK",
        DEFAULT_ENABLE_FALLBACK,
    )?;

    let workflow_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
    let workflow = load_workflow(&workflow_path)?;
    let mut runtime = FlashCaptureRuntime {
        logger,
        opts,
        repo_dir,
        outputs,
        port,
        _port_lock: Some(port_lock),
        baud,
        flash_baud,
        fallback_baud,
        flash_timeout,
        flash_status_interval,
        flash_idle_timeout,
        flash_progress_stall_timeout,
        flash_log_drain_timeout,
        boot_window,
        skip_update_check,
        image_path: None,
        idf_env: None,
        flash_result: None,
        capture_bytes: 0,
    };

    let flash_mode = match runtime.opts.flash_mode {
        FlashMode::Auto => "auto",
        FlashMode::Full => "full",
        FlashMode::AppOnly => "app-only",
    };
    let capture_mode = match runtime.opts.capture_mode {
        CaptureMode::Boot => "boot",
        CaptureMode::Stream => "stream",
        CaptureMode::None => "none",
    };
    let workflow_input = json!({
        "flash_mode": flash_mode,
        "capture_mode": capture_mode,
        "image_supplied": runtime.opts.image.is_some(),
        "set_time_after_flash": set_time_after_flash,
        "fallback_allowed": enable_fallback,
    });
    execute_workflow(&workflow, &mut runtime, &workflow_input)?;

    let flash_result = runtime
        .flash_result
        .as_ref()
        .ok_or_else(|| anyhow!("flash workflow completed without a flash result"))?;
    runtime.logger.info(format!(
        "flash-capture complete: port={} strategy={:?} artifacts={}",
        runtime.port,
        flash_result.strategy,
        runtime.outputs.root.display()
    ));
    Ok(())
}

impl FlashCaptureRuntime<'_> {
    fn action_preflight(&mut self, context: &mut Value) -> Result<()> {
        let fallback_allowed = context
            .get("fallback_allowed")
            .and_then(Value::as_bool)
            .unwrap_or(DEFAULT_ENABLE_FALLBACK);
        self.logger.info(format!(
            "Starting flash-capture: port={} profile={} flash_mode={:?} capture_mode={:?} fallback_allowed={}",
            self.port,
            self.opts.profile,
            self.opts.flash_mode,
            self.opts.capture_mode,
            fallback_allowed
        ));
        File::create(&self.outputs.capture_log)?;
        Ok(())
    }

    fn action_resolve_image(&mut self, args: &Value) -> Result<()> {
        let source = args
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("resolve_image requires source"))?;
        let image_path = match source {
            "explicit" => resolve_explicit_image(self.opts.image.as_deref(), &self.repo_dir)?,
            "build" => build_firmware_image(
                self.logger,
                &self.opts.profile,
                &self.outputs.flash_log,
                &self.repo_dir,
            )?,
            other => bail!("unsupported image source `{other}`"),
        };
        self.image_path = Some(image_path);
        Ok(())
    }

    fn action_prepare_idf_env(&mut self) -> Result<()> {
        let idf_env = bootstrap_idf_env(
            self.opts.idf_root.as_deref(),
            self.opts.idf_tools_path.as_deref(),
        )?;
        self.idf_env = Some(idf_env);
        Ok(())
    }

    fn action_flash(&mut self, args: &Value) -> Result<()> {
        let image_path = self
            .image_path
            .as_deref()
            .ok_or_else(|| anyhow!("image_path not resolved before flash"))?;
        let strategy = args
            .get("strategy")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("flash requires strategy"))?;
        let fallback_used = args
            .get("fallback_used")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut result = match strategy {
            "full" => run_full_flash(FullFlashOptions {
                image_path,
                flash_log: &self.outputs.flash_log,
                port: &self.port,
                flash_baud: self.flash_baud,
                flash_timeout: self.flash_timeout,
                flash_status_interval: self.flash_status_interval,
                skip_update_check: self.skip_update_check,
            })?,
            "app-only" => run_app_only_flash(AppOnlyFlashOptions {
                image_path,
                flash_log: &self.outputs.flash_log,
                port: &self.port,
                flash_baud: self.fallback_baud,
                flash_timeout: self.flash_timeout,
                flash_status_interval: self.flash_status_interval,
                flash_idle_timeout: self.flash_idle_timeout,
                flash_progress_stall_timeout: self.flash_progress_stall_timeout,
                flash_log_drain_timeout: self.flash_log_drain_timeout,
                skip_update_check: self.skip_update_check,
                idf_env: self.idf_env.as_ref(),
            })?,
            other => bail!("unsupported flash strategy `{other}`"),
        };
        result.fallback_used = fallback_used;
        self.flash_result = Some(result);
        Ok(())
    }

    fn action_capture(&mut self, args: &Value, context: &mut Value) -> Result<()> {
        let mode = args
            .get("mode")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("capture requires mode"))?;
        match mode {
            "boot" => {
                self.logger.info(format!(
                    "capturing boot log for {} ms on {} -> {}",
                    self.boot_window.as_millis(),
                    self.port,
                    self.outputs.capture_log.display()
                ));
                let bytes = capture_boot_window(
                    &self.port,
                    self.baud,
                    self.boot_window,
                    &self.outputs.capture_log,
                )?;
                self.capture_bytes = bytes.len();
                self.logger.info(format!(
                    "boot capture complete: {} bytes -> {}",
                    self.capture_bytes,
                    self.outputs.capture_log.display()
                ));
            }
            "stream" => {
                self.logger.info(format!(
                    "capturing serial stream for {} ms on {} -> {}",
                    self.boot_window.as_millis(),
                    self.port,
                    self.outputs.capture_log.display()
                ));
                let bytes = capture_stream_window(
                    &self.port,
                    self.baud,
                    self.boot_window,
                    &self.outputs.capture_log,
                )?;
                self.capture_bytes = bytes.len();
                self.logger.info(format!(
                    "stream capture complete: {} bytes -> {}",
                    self.capture_bytes,
                    self.outputs.capture_log.display()
                ));
            }
            "none" => {
                File::create(&self.outputs.capture_log)?;
                self.capture_bytes = 0;
                self.logger.info(format!(
                    "capture skipped; created empty {}",
                    self.outputs.capture_log.display()
                ));
            }
            other => bail!("unsupported capture mode `{other}`"),
        }
        context_set_u64(context, "capture_bytes", self.capture_bytes as u64);
        Ok(())
    }
}

impl WorkflowRuntime for FlashCaptureRuntime<'_> {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "preflight" => self.action_preflight(context),
            "resolve_image" => self.action_resolve_image(args),
            "prepare_idf_env" => self.action_prepare_idf_env(),
            "flash" => self.action_flash(args),
            "capture" => self.action_capture(args, context),
            "post_flash_timeset" => run_timeset_after_flash(self.logger, &self.port, self.baud),
            "write_summary" => action_write_summary(self, context),
            "abort_flash" => action_abort_flash(context),
            other => Err(anyhow!(
                "unsupported flash-capture workflow action: {other}"
            )),
        }
    }
}
