use super::artifacts::{
    archive_firmware_artifacts, validate_post_command_options, ArchiveFirmwareArtifactsOptions,
};
use super::flash::{run_app_only_flash, run_full_flash, AppOnlyFlashOptions, FullFlashOptions};
use super::paths::{
    acquire_port_lock, build_firmware_image, build_single_production_bootloader_command,
    ensure_port_available, normalize_output_root, prepare_output_paths, resolve_explicit_image,
    resolve_port, write_boot_selection,
};
use super::runtime_helpers::{context_set_bool, context_set_string};
use super::{
    BootTarget, CaptureMode, FlashCaptureOptions, FlashCaptureRuntime, FlashMode, TimeSyncStatus,
    DEFAULT_ENABLE_FALLBACK, DEFAULT_FLASH_BAUD,
};
use std::{env, fs::File, path::PathBuf, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::{
    env_utils,
    idf_env::bootstrap_idf_env,
    logging::Logger,
    scenarios::{execute_workflow, load_workflow},
    serial_console::SerialConsole,
    workflows::common::repo_root,
};

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
    let flash_baud = opts.flash_baud.unwrap_or(env_utils::parse_env_u32(
        "ESPFLASH_BAUD",
        DEFAULT_FLASH_BAUD,
    )?);
    let fallback_baud = env_utils::parse_env_u32("ESPFLASH_FALLBACK_BAUD", 115_200)?;
    let flash_timeout = Duration::from_secs(env_utils::parse_env_u64("FLASH_TIMEOUT_SEC", 360)?);
    let flash_status_interval =
        Duration::from_secs(env_utils::parse_env_u64("FLASH_STATUS_INTERVAL_SEC", 15)?);
    let flash_idle_timeout =
        Duration::from_secs(env_utils::parse_env_u64("FLASH_IDLE_TIMEOUT_SEC", 45)?);
    let flash_progress_stall_timeout = Duration::from_secs(env_utils::parse_env_u64(
        "FLASH_PROGRESS_STALL_TIMEOUT_SEC",
        30,
    )?);
    let flash_log_drain_timeout = Duration::from_millis(env_utils::parse_env_u64(
        "FLASH_LOG_DRAIN_TIMEOUT_MS",
        1_000,
    )?);
    let boot_window = Duration::from_millis(opts.boot_window_ms.unwrap_or(8_000));
    let skip_update_check = env_utils::parse_env_bool01("ESPFLASH_SKIP_UPDATE_CHECK", true)?;
    let enable_fallback =
        env_utils::parse_env_bool01("ESPFLASH_ENABLE_FALLBACK", DEFAULT_ENABLE_FALLBACK)?;
    // `--no-time-sync` wins outright; only in its absence does the
    // `FLASH_SET_TIME_AFTER_FLASH=0` compatibility override apply.
    let time_sync_enabled = if opts.no_time_sync {
        false
    } else {
        env_utils::parse_env_bool01("FLASH_SET_TIME_AFTER_FLASH", true)?
    };

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
        ota_data_path: None,
        image_built_in_workflow: false,
        idf_env: None,
        flash_result: None,
        capture_bytes: 0,
        capture_console: None,
        post_command_match: None,
        time_sync_status: TimeSyncStatus::Skipped,
        time_sync_requested_utc: None,
        time_sync_requested_offset_minutes: None,
        time_sync_utc: None,
        time_sync_offset_minutes: None,
        time_sync_reason: None,
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
    let boot_target = runtime.opts.boot_target.label();
    let workflow_input = json!({
        "flash_mode": flash_mode,
        "boot_target": boot_target,
        "capture_mode": capture_mode,
        "reset_after_flash": runtime.opts.capture_mode != CaptureMode::Boot,
        "image_supplied": runtime.opts.image.is_some(),
        "fallback_allowed": enable_fallback,
        "post_command_supplied": runtime.opts.post_command.is_some(),
        "time_sync_enabled": time_sync_enabled,
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
    pub(super) fn action_preflight(&mut self, context: &mut Value) -> Result<()> {
        validate_post_command_options(&self.opts)?;
        let fallback_allowed = context
            .get("fallback_allowed")
            .and_then(Value::as_bool)
            .unwrap_or(DEFAULT_ENABLE_FALLBACK);
        self.logger.info(format!(
            "Starting flash-capture: port={} profile={} flash_mode={:?} boot_target={:?} capture_mode={:?} fallback_allowed={}",
            self.port,
            self.opts.profile,
            self.opts.flash_mode,
            self.opts.boot_target,
            self.opts.capture_mode,
            fallback_allowed
        ));
        File::create(&self.outputs.capture_log)?;
        Ok(())
    }

    pub(super) fn action_resolve_image(&mut self, args: &Value) -> Result<()> {
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
        self.image_built_in_workflow = source == "build";
        self.image_path = Some(image_path);
        Ok(())
    }

    pub(super) fn action_prepare_idf_env(&mut self) -> Result<()> {
        let idf_env = bootstrap_idf_env(
            self.opts.idf_root.as_deref(),
            self.opts.idf_tools_path.as_deref(),
        )?;
        self.idf_env = Some(idf_env);
        Ok(())
    }

    pub(super) fn action_prepare_flash_layout(&mut self, args: &Value) -> Result<()> {
        let boot_target = match args.get("boot_target").and_then(Value::as_str) {
            Some("factory") => BootTarget::Factory,
            Some("production") => BootTarget::Production,
            Some(other) => bail!("unsupported boot target `{other}`"),
            None => bail!("prepare_flash_layout requires boot_target"),
        };
        self.logger
            .info("building pinned single-production bootloader (ESP-IDF v5.5.2)");
        let command = build_single_production_bootloader_command(&self.repo_dir)?;
        let status = super::command_run::run_command_logged(
            &command,
            &self.outputs.flash_log,
            super::command_run::CommandRunOptions::new(super::DEFAULT_LOG_DRAIN_TIMEOUT),
        )?;
        if !status.success() {
            bail!(
                "single-production bootloader build failed; see {}",
                self.outputs.flash_log.display()
            );
        }
        let ota_data_path = write_boot_selection(&self.repo_dir, boot_target)?;
        self.logger.info(format!(
            "prepared {} boot selection at {}",
            boot_target.label(),
            ota_data_path.display()
        ));
        self.ota_data_path = Some(ota_data_path);
        Ok(())
    }

    pub(super) fn action_archive_image(&mut self, args: &Value) -> Result<()> {
        let include_bootloader = args
            .get("include_bootloader")
            .and_then(Value::as_bool)
            .ok_or_else(|| anyhow!("archive_image requires include_bootloader"))?;
        let image_path = self
            .image_path
            .as_deref()
            .ok_or_else(|| anyhow!("image_path not resolved before archive"))?;
        archive_firmware_artifacts(ArchiveFirmwareArtifactsOptions {
            image_path,
            outputs: &self.outputs,
            repo_dir: &self.repo_dir,
            profile: &self.opts.profile,
            skip_update_check: self.skip_update_check,
            include_bootloader,
            built_in_workflow: self.image_built_in_workflow,
        })
    }

    pub(super) fn action_post_command(&mut self, context: &mut Value) -> Result<()> {
        let command = self
            .opts
            .post_command
            .as_deref()
            .ok_or_else(|| anyhow!("post_command action requires --post-command"))?;
        let pattern = self
            .opts
            .post_pattern
            .as_deref()
            .ok_or_else(|| anyhow!("--post-pattern is required with --post-command"))?;
        let timeout_ms = self.opts.post_timeout_ms.unwrap_or(120_000);
        let settle_ms = crate::workflows::serial::CONSOLE_SETTLE_MS;
        let regex = regex::Regex::new(pattern)
            .with_context(|| format!("invalid --post-pattern `{pattern}`"))?;
        // Capture/time sync retains its handle. End that ownership before the
        // separately logged command session, without toggling the reset lines.
        drop(self.capture_console.take());
        let mut console = SerialConsole::open_passive(
            &self.port,
            self.baud,
            Some(&self.outputs.post_command_log),
        )?;
        console.settle(settle_ms)?;
        let mark = console.mark();
        console.send_line(command)?;
        let matched = console
            .wait_for_regex_since(mark, &regex, Duration::from_millis(timeout_ms))?
            .ok_or_else(|| {
                anyhow!(
                    "post command `{command}` did not match `{pattern}` within {timeout_ms} ms; see {}",
                    self.outputs.post_command_log.display()
                )
            })?;
        self.logger
            .info(format!("post command completed: {matched}"));
        context_set_string(context, "post_command_match", &matched);
        self.post_command_match = Some(matched);
        Ok(())
    }

    /// Runs the shared `hostctl timeset` readiness policy against the
    /// just-flashed device. Never returns `Err`: a failure is recorded into
    /// both `self` (for `write_summary`) and the workflow `context` (for the
    /// `time_sync_result_gate`/`fail_time_sync` branch that runs *after* the
    /// summary is written), rather than aborting the workflow here.
    pub(super) fn action_time_sync(&mut self, context: &mut Value) -> Result<()> {
        let enabled = context
            .get("time_sync_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if !enabled {
            self.time_sync_status = TimeSyncStatus::Skipped;
            context_set_bool(context, "time_sync_ok", true);
            context_set_string(context, "time_sync_status", TimeSyncStatus::Skipped.label());
            return Ok(());
        }

        let settle_ms = crate::workflows::serial::CONSOLE_SETTLE_MS;
        // The boot/stream capture that already ran (`action_capture`) is the
        // device's only chance to be heard: its `TIME_REQUEST` prints once,
        // very early in boot, and a fresh console opened only now -- after
        // capture closed its own connection -- would never see it live. Read
        // it back out of the capture log instead; only fall back to a live
        // wait (`capture_mode: "none"`, or a request the capture window
        // somehow missed) when there is nothing captured to read.
        // A captured request is only useful if the capture window itself
        // didn't already run longer than the session's own deadline -- past
        // that point the device's Coordinator has certainly already closed
        // the session, and replying to it would just wait out a doomed
        // timeout. Discard it in that case rather than trying anyway.
        let known_request = super::capture::find_captured_request(&self.outputs.capture_log)
            .filter(|request| {
                (self.boot_window.as_millis() as u64) < u64::from(request.deadline_ms)
            });
        let console_result = self
            .capture_console
            .take()
            .map_or_else(|| SerialConsole::open(&self.port, self.baud, None), Ok);
        let sync_result = console_result.and_then(|mut console| {
            let sync_mark = console.mark();
            let result = (|| {
                console.settle(settle_ms)?;
                if known_request.is_none() {
                    // Nothing usable was captured (`capture_mode: "none"`,
                    // or a session that's certainly already closed) -- the
                    // device finished booting long ago and is listening on
                    // its permanent UART task either way, so ask for a
                    // fresh manual-demand session instead of hoping a live
                    // wait catches one that already expired.
                    console.send_line("TIMESYNC")?;
                }
                crate::workflows::serial::sync_time(
                    &mut console,
                    known_request,
                    crate::workflows::serial::NO_CAPTURED_REQUEST_WAIT_MS,
                )
            })();
            if let Err(error) = super::capture::append_time_sync_diagnostics(
                &self.outputs.capture_log,
                &console.read_recent_lines(sync_mark),
            ) {
                self.logger
                    .warn(format!("could not retain time sync diagnostics: {error}"));
            }
            self.capture_console = Some(console);
            result
        });
        match sync_result {
            Ok(outcome) => {
                self.time_sync_status = TimeSyncStatus::Ok;
                self.time_sync_requested_utc = Some(outcome.requested_utc_epoch_seconds);
                self.time_sync_requested_offset_minutes = Some(outcome.requested_offset_minutes);
                self.time_sync_utc = Some(outcome.verified_utc_epoch_seconds);
                self.time_sync_offset_minutes = Some(outcome.verified_offset_minutes);
                self.logger.info(format!(
                    "time sync OK requested_utc={} requested_offset_min={} verified_utc={} verified_offset_min={}",
                    outcome.requested_utc_epoch_seconds,
                    outcome.requested_offset_minutes,
                    outcome.verified_utc_epoch_seconds,
                    outcome.verified_offset_minutes,
                ));
                context_set_bool(context, "time_sync_ok", true);
                context_set_string(context, "time_sync_status", TimeSyncStatus::Ok.label());
            }
            Err(error) => {
                self.time_sync_status = TimeSyncStatus::Failed;
                self.time_sync_reason = Some(error.to_string());
                self.logger.warn(format!("time sync failed: {error}"));
                context_set_bool(context, "time_sync_ok", false);
                context_set_string(context, "time_sync_status", TimeSyncStatus::Failed.label());
                context_set_string(context, "time_sync_reason", &error.to_string());
            }
        }
        Ok(())
    }

    pub(super) fn action_flash(&mut self, args: &Value) -> Result<()> {
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
        let reset_after_flash = args
            .get("reset_after_flash")
            .and_then(Value::as_bool)
            .ok_or_else(|| anyhow!("flash requires reset_after_flash"))?;
        let ota_data_path = self.ota_data_path.as_deref();

        let mut result = match strategy {
            "full" => run_full_flash(FullFlashOptions {
                repo_dir: &self.repo_dir,
                image_path: &self.outputs.app_bin,
                ota_data_path: ota_data_path
                    .ok_or_else(|| anyhow!("boot selection not prepared before full flash"))?,
                flash_log: &self.outputs.flash_log,
                port: &self.port,
                flash_baud: self.flash_baud,
                no_stub: false,
                reset_after_flash,
                flash_timeout: self.flash_timeout,
                flash_status_interval: self.flash_status_interval,
                flash_idle_timeout: self.flash_idle_timeout,
                flash_progress_stall_timeout: self.flash_progress_stall_timeout,
                flash_log_drain_timeout: self.flash_log_drain_timeout,
                idf_env: self.idf_env.as_ref(),
            })?,
            "full-safe" => run_full_flash(FullFlashOptions {
                repo_dir: &self.repo_dir,
                image_path: &self.outputs.app_bin,
                ota_data_path: ota_data_path
                    .ok_or_else(|| anyhow!("boot selection not prepared before full flash"))?,
                flash_log: &self.outputs.flash_log,
                port: &self.port,
                flash_baud: self.fallback_baud,
                no_stub: true,
                reset_after_flash,
                flash_timeout: self.flash_timeout,
                flash_status_interval: self.flash_status_interval,
                flash_idle_timeout: self.flash_idle_timeout,
                flash_progress_stall_timeout: self.flash_progress_stall_timeout,
                flash_log_drain_timeout: self.flash_log_drain_timeout,
                idf_env: self.idf_env.as_ref(),
            })?,
            "app-only" => run_app_only_flash(AppOnlyFlashOptions {
                repo_dir: &self.repo_dir,
                image_path,
                flash_log: &self.outputs.flash_log,
                port: &self.port,
                flash_baud: self.fallback_baud,
                reset_after_flash,
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
}
