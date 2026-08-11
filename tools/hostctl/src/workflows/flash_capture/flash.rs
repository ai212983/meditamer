use super::command_run::{run_command_logged, CommandRunOptions};
use super::{
    CommandProgressMode, CommandSpec, FlashResult, FlashStrategy, DEFAULT_LOG_DRAIN_TIMEOUT,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, bail, Result};

use crate::idf_env::IdfEnv;

pub(super) struct FullFlashOptions<'a> {
    pub(super) image_path: &'a Path,
    pub(super) flash_log: &'a Path,
    pub(super) port: &'a str,
    pub(super) flash_baud: u32,
    pub(super) flash_timeout: Duration,
    pub(super) flash_status_interval: Duration,
    pub(super) skip_update_check: bool,
}

pub(super) struct AppOnlyFlashOptions<'a> {
    pub(super) image_path: &'a Path,
    pub(super) flash_log: &'a Path,
    pub(super) port: &'a str,
    pub(super) flash_baud: u32,
    pub(super) flash_timeout: Duration,
    pub(super) flash_status_interval: Duration,
    pub(super) flash_idle_timeout: Duration,
    pub(super) flash_progress_stall_timeout: Duration,
    pub(super) flash_log_drain_timeout: Duration,
    pub(super) skip_update_check: bool,
    pub(super) idf_env: Option<&'a IdfEnv>,
}

pub(super) fn run_full_flash(opts: FullFlashOptions<'_>) -> Result<FlashResult> {
    let mut command = CommandSpec::new("espflash")
        .args(["flash", "-c", "esp32", "-B"])
        .arg(opts.flash_baud.to_string())
        .args(["-p", opts.port]);
    if opts.skip_update_check {
        command = command.arg("--skip-update-check");
    }
    command = command.arg(opts.image_path.as_os_str());
    let status = run_command_logged(
        &command,
        opts.flash_log,
        CommandRunOptions {
            timeout: Some(opts.flash_timeout),
            progress_interval: Some(opts.flash_status_interval),
            progress_label: Some("full flash"),
            ..CommandRunOptions::new(DEFAULT_LOG_DRAIN_TIMEOUT)
        },
    )?;
    if !status.success() {
        bail!("espflash full-flash failed with status {status}");
    }
    Ok(FlashResult {
        strategy: FlashStrategy::Full,
        image_path: opts.image_path.to_path_buf(),
        fallback_used: false,
        python_bin: None,
        idf_root: None,
        idf_py_bin: None,
    })
}

pub(super) fn run_app_only_flash(opts: AppOnlyFlashOptions<'_>) -> Result<FlashResult> {
    let idf_env = opts
        .idf_env
        .ok_or_else(|| anyhow!("ESP-IDF env is required for app-only flash"))?;
    let app_bin = if opts.image_path.extension().and_then(|ext| ext.to_str()) == Some("bin") {
        opts.image_path.to_path_buf()
    } else {
        build_app_binary(opts.image_path, opts.flash_log, opts.skip_update_check)?
    };

    let command = build_app_flash_command(idf_env, opts.port, opts.flash_baud, &app_bin);
    let timeout = opts
        .flash_timeout
        .checked_mul(2)
        .unwrap_or(Duration::from_secs(u64::MAX));
    let status = run_command_logged(
        &command,
        opts.flash_log,
        CommandRunOptions {
            timeout: Some(timeout),
            progress_interval: Some(opts.flash_status_interval),
            progress_label: Some("app-only flash"),
            idle_timeout: Some(opts.flash_idle_timeout),
            progress_stall_timeout: Some(opts.flash_progress_stall_timeout),
            progress_mode: Some(CommandProgressMode::EsptoolWriteFlash),
            log_drain_timeout: opts.flash_log_drain_timeout,
        },
    )?;
    if !status.success() {
        bail!("app-only esptool flash failed with status {status}");
    }
    Ok(FlashResult {
        strategy: FlashStrategy::AppOnly,
        image_path: app_bin,
        fallback_used: false,
        python_bin: Some(idf_env.python_bin.clone()),
        idf_root: Some(idf_env.idf_root.clone()),
        idf_py_bin: idf_env.idf_py_bin.clone(),
    })
}

pub(super) fn build_app_binary(
    image_path: &Path,
    flash_log: &Path,
    skip_update_check: bool,
) -> Result<PathBuf> {
    let generated = flash_log
        .parent()
        .expect("flash_log parent")
        .join("app.bin");
    let mut save_image = CommandSpec::new("espflash")
        .args(["save-image", "--chip", "esp32"])
        .arg(image_path.as_os_str())
        .arg(generated.as_os_str());
    if skip_update_check {
        save_image = save_image.arg("--skip-update-check");
    }
    let status = run_command_logged(
        &save_image,
        flash_log,
        CommandRunOptions::new(DEFAULT_LOG_DRAIN_TIMEOUT),
    )?;
    if !status.success() {
        bail!("espflash save-image failed with status {status}");
    }
    if generated.is_file() {
        Ok(generated)
    } else {
        bail!(
            "espflash save-image did not produce {}",
            generated.display()
        );
    }
}

pub fn build_app_flash_command(
    idf_env: &IdfEnv,
    port: &str,
    flash_baud: u32,
    app_bin: &Path,
) -> CommandSpec {
    CommandSpec::new(idf_env.python_bin.as_os_str())
        .arg(idf_env.esptool_bin.as_os_str())
        .args(["--chip", "esp32", "--port", port, "--baud"])
        .arg(flash_baud.to_string())
        .arg("--no-stub")
        .args(["write_flash", "-z", "0x10000"])
        .arg(app_bin.as_os_str())
}
