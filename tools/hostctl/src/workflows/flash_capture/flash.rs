use super::command_run::{run_command_logged, CommandRunOptions};
use super::{
    CommandProgressMode, CommandSpec, FlashResult, FlashStrategy, DEFAULT_LOG_DRAIN_TIMEOUT,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};

use crate::idf_env::IdfEnv;

pub(super) struct FullFlashOptions<'a> {
    pub(super) repo_dir: &'a Path,
    pub(super) image_path: &'a Path,
    pub(super) flash_log: &'a Path,
    pub(super) port: &'a str,
    pub(super) flash_baud: u32,
    pub(super) no_stub: bool,
    pub(super) flash_timeout: Duration,
    pub(super) flash_status_interval: Duration,
    pub(super) flash_idle_timeout: Duration,
    pub(super) flash_progress_stall_timeout: Duration,
    pub(super) flash_log_drain_timeout: Duration,
    pub(super) idf_env: Option<&'a IdfEnv>,
}

pub(super) struct AppOnlyFlashOptions<'a> {
    pub(super) repo_dir: &'a Path,
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

pub(super) struct FullFlashCommandOptions<'a> {
    pub(super) idf_env: &'a IdfEnv,
    pub(super) port: &'a str,
    pub(super) flash_baud: u32,
    pub(super) no_stub: bool,
    pub(super) bootloader: &'a Path,
    pub(super) partition_table: &'a Path,
    pub(super) ota_data_offset: u32,
    pub(super) ota_data: &'a Path,
    pub(super) app_offset: u32,
    pub(super) app_bin: &'a Path,
}

pub(super) fn run_full_flash(opts: FullFlashOptions<'_>) -> Result<FlashResult> {
    let idf_env = opts
        .idf_env
        .ok_or_else(|| anyhow!("ESP-IDF env is required for full flash"))?;
    let bootloader = opts
        .repo_dir
        .join("target/single-production-bootloader/bootloader/bootloader.bin");
    let partition_csv = opts
        .repo_dir
        .join("config/partitions-single-production.csv");
    let partition_table = opts
        .repo_dir
        .join("target/single-production-bootloader/partition_table/partition-table.bin");
    let ota_data = opts
        .repo_dir
        .join("target/single-production-bootloader/ota_data_initial.bin");
    if !bootloader.is_file()
        || !partition_csv.is_file()
        || !partition_table.is_file()
        || !ota_data.is_file()
    {
        bail!("pinned single-production bootloader or partition table is missing");
    }
    let app_offset = resolve_partition_offset(&partition_csv, "ota_0")?;
    let ota_data_offset = resolve_partition_offset(&partition_csv, "otadata")?;
    let command = build_full_flash_command(FullFlashCommandOptions {
        idf_env,
        port: opts.port,
        flash_baud: opts.flash_baud,
        no_stub: opts.no_stub,
        bootloader: &bootloader,
        partition_table: &partition_table,
        ota_data_offset,
        ota_data: &ota_data,
        app_offset,
        app_bin: opts.image_path,
    });
    let status = run_command_logged(
        &command,
        opts.flash_log,
        CommandRunOptions {
            timeout: Some(opts.flash_timeout),
            progress_interval: Some(opts.flash_status_interval),
            progress_label: Some("full flash"),
            idle_timeout: Some(opts.flash_idle_timeout),
            progress_stall_timeout: Some(opts.flash_progress_stall_timeout),
            progress_mode: Some(CommandProgressMode::EsptoolWriteFlash),
            log_drain_timeout: opts.flash_log_drain_timeout,
        },
    )?;
    if !status.success() {
        bail!("espflash full-flash failed with status {status}");
    }
    Ok(FlashResult {
        strategy: FlashStrategy::Full,
        image_path: opts.image_path.to_path_buf(),
        fallback_used: false,
        python_bin: Some(idf_env.python_bin.clone()),
        idf_root: Some(idf_env.idf_root.clone()),
        idf_py_bin: idf_env.idf_py_bin.clone(),
    })
}

pub(super) fn build_full_flash_command(opts: FullFlashCommandOptions<'_>) -> CommandSpec {
    let mut command = CommandSpec::new(opts.idf_env.python_bin.as_os_str())
        .arg(opts.idf_env.esptool_bin.as_os_str())
        .args(["--chip", "esp32", "--port", opts.port, "--baud"])
        .arg(opts.flash_baud.to_string());
    if opts.no_stub {
        command = command.arg("--no-stub");
    }
    command
        .args([
            "write_flash",
            "-z",
            "--flash_mode",
            "dio",
            "--flash_freq",
            "40m",
            "--flash_size",
            "4MB",
            "0x1000",
        ])
        .arg(opts.bootloader.as_os_str())
        .arg("0x8000")
        .arg(opts.partition_table.as_os_str())
        .arg(format!("0x{:x}", opts.ota_data_offset))
        .arg(opts.ota_data.as_os_str())
        .arg(format!("0x{:x}", opts.app_offset))
        .arg(opts.app_bin.as_os_str())
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

    let partition_table = opts
        .repo_dir
        .join("config/partitions-single-production.csv");
    let app_offset = resolve_partition_offset(&partition_table, "ota_0")?;
    let command =
        build_app_flash_command(idf_env, opts.port, opts.flash_baud, app_offset, &app_bin);
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
    app_offset: u32,
    app_bin: &Path,
) -> CommandSpec {
    CommandSpec::new(idf_env.python_bin.as_os_str())
        .arg(idf_env.esptool_bin.as_os_str())
        .args(["--chip", "esp32", "--port", port, "--baud"])
        .arg(flash_baud.to_string())
        .arg("--no-stub")
        .args(["write_flash", "-z"])
        .arg(format!("0x{app_offset:x}"))
        .arg(app_bin.as_os_str())
}

pub fn resolve_partition_offset(path: &Path, label: &str) -> Result<u32> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("read partition table {}", path.display()))?;
    for line in contents.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let columns: Vec<_> = line.split(',').map(str::trim).collect();
        if columns.first().copied() != Some(label) {
            continue;
        }
        let raw = columns
            .get(3)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("partition `{label}` has no explicit offset"))?;
        let offset = raw
            .strip_prefix("0x")
            .or_else(|| raw.strip_prefix("0X"))
            .map_or_else(|| raw.parse::<u32>(), |hex| u32::from_str_radix(hex, 16))
            .with_context(|| format!("invalid offset `{raw}` for partition `{label}`"))?;
        return Ok(offset);
    }
    bail!("partition `{label}` not found in {}", path.display())
}
