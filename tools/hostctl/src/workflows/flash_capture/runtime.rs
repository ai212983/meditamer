use super::artifacts::{
    archive_firmware_artifacts, validate_post_command_options, ArchiveFirmwareArtifactsOptions,
};
use super::flash::{run_app_only_flash, run_full_flash, AppOnlyFlashOptions, FullFlashOptions};
use super::paths::{
    acquire_port_lock, build_firmware_image, build_ota_bootloader_command, ensure_port_available,
    normalize_output_root, prepare_output_paths, resolve_explicit_image, resolve_port,
};
use super::runtime_helpers::context_set_string;
use super::{
    CaptureMode, FlashCaptureOptions, FlashCaptureRuntime, FlashMode, DEFAULT_ENABLE_FALLBACK,
    DEFAULT_FLASH_BAUD,
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
    let boot_window = Duration::from_millis(opts.boot_window_ms.unwrap_or(
        env_utils::parse_env_u64("HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS", 8_000)?,
    ));
    let skip_update_check = env_utils::parse_env_bool01("ESPFLASH_SKIP_UPDATE_CHECK", true)?;
    let enable_fallback =
        env_utils::parse_env_bool01("ESPFLASH_ENABLE_FALLBACK", DEFAULT_ENABLE_FALLBACK)?;

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
        post_command_match: None,
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
        "fallback_allowed": enable_fallback,
        "post_command_supplied": runtime.opts.post_command.is_some(),
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

    pub(super) fn action_prepare_bootloader(&mut self) -> Result<()> {
        self.logger
            .info("building pinned OTA bootloader (ESP-IDF v5.5.2)");
        let command = build_ota_bootloader_command(&self.repo_dir)?;
        let status = super::command_run::run_command_logged(
            &command,
            &self.outputs.flash_log,
            super::command_run::CommandRunOptions::new(super::DEFAULT_LOG_DRAIN_TIMEOUT),
        )?;
        if !status.success() {
            bail!(
                "OTA bootloader build failed; see {}",
                self.outputs.flash_log.display()
            );
        }
        Ok(())
    }

    pub(super) fn action_archive_image(&mut self) -> Result<()> {
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
        let settle_ms = env_utils::parse_env_u64("HOSTCTL_FLASH_CAPTURE_POST_SETTLE_MS", 200)?;
        let regex = regex::Regex::new(pattern)
            .with_context(|| format!("invalid --post-pattern `{pattern}`"))?;
        let mut console =
            SerialConsole::open(&self.port, self.baud, Some(&self.outputs.post_command_log))?;
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

        let mut result = match strategy {
            "full" => run_full_flash(FullFlashOptions {
                repo_dir: &self.repo_dir,
                image_path: &self.outputs.app_bin,
                flash_log: &self.outputs.flash_log,
                port: &self.port,
                flash_baud: self.flash_baud,
                no_stub: false,
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
                flash_log: &self.outputs.flash_log,
                port: &self.port,
                flash_baud: self.fallback_baud,
                no_stub: true,
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
