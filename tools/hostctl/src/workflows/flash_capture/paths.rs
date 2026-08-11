use super::command_run::{run_command_logged, CommandRunOptions};
use super::{CommandSpec, OutputPaths, PortLock, DEFAULT_LOG_DRAIN_TIMEOUT};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{anyhow, bail, Result};
use chrono::Local;
use fs2::FileExt;

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    port_detect,
    workflows::common::repo_root,
};

pub fn prepare_output_paths(explicit: Option<&Path>) -> Result<OutputPaths> {
    let root = match explicit {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => repo_root().join(path),
        None => repo_root().join("logs").join(format!(
            "flash_capture_{}",
            Local::now().format("%Y%m%d_%H%M%S")
        )),
    };
    fs::create_dir_all(&root)?;
    Ok(OutputPaths {
        flash_log: root.join("flash.log"),
        capture_log: root.join("capture.log"),
        post_command_log: root.join("post-command.log"),
        summary: root.join("summary.txt"),
        firmware_elf: root.join("firmware.elf"),
        app_bin: root.join("app.bin"),
        bootloader_bin: root.join("bootloader.bin"),
        partition_table_bin: root.join("partition-table.bin"),
        hashes: root.join("sha256.txt"),
        build_metadata: root.join("build-metadata.txt"),
        root,
    })
}

pub(super) fn build_ota_bootloader_command(repo_dir: &Path) -> Result<CommandSpec> {
    let script = repo_dir.join("scripts/build/ota_bootloader.sh");
    if !script.is_file() {
        bail!("missing OTA bootloader build script {}", script.display());
    }
    Ok(CommandSpec::new(script.as_os_str()).current_dir(repo_dir))
}

pub fn normalize_output_root(explicit: Option<&Path>) -> (Option<PathBuf>, Option<String>) {
    let Some(path) = explicit else {
        return (None, None);
    };

    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return (Some(path.to_path_buf()), None);
    };

    let looks_like_file = matches!(file_name, "flash.log" | "capture.log" | "summary.txt")
        || path.extension().and_then(|ext| ext.to_str()) == Some("log");
    if !looks_like_file {
        return (Some(path.to_path_buf()), None);
    }

    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return (Some(path.to_path_buf()), None);
    };

    (
        Some(parent.to_path_buf()),
        Some(format!(
            "treating file-like --log path {} as artifact directory {}",
            path.display(),
            parent.display()
        )),
    )
}

pub(super) fn resolve_port(logger: &mut Logger, explicit: Option<&str>) -> Result<String> {
    if let Some(port) = explicit.filter(|port| !port.trim().is_empty()) {
        let canonical = port_detect::canonicalize_port(port);
        if canonical != port {
            logger.warn(format!(
                "rewriting explicit serial port {} to preferred {}",
                port, canonical
            ));
        }
        return Ok(canonical);
    }
    env_utils::require_port()
}

pub fn acquire_port_lock(port: &str) -> Result<PortLock> {
    let lock_name = port.replace('/', "_");
    let path = repo_root()
        .join("logs/.state")
        .join(format!("flash_capture{lock_name}.lock"));
    ensure_parent_dir(&path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .read(true)
        .open(path)?;
    file.try_lock_exclusive().map_err(|err| {
        anyhow!("another repo flash/capture session is already using {port}: {err}")
    })?;
    writeln!(file, "pid={}", std::process::id())?;
    writeln!(file, "port={port}")?;
    Ok(PortLock { file })
}

pub(super) fn ensure_port_available(port: &str) -> Result<()> {
    let output = match Command::new("lsof").arg(port).output() {
        Ok(output) => output,
        Err(_) => return Ok(()),
    };
    if output.status.success() && !output.stdout.is_empty() {
        bail!(
            "serial port is busy: {}\n{}",
            port,
            String::from_utf8_lossy(&output.stdout)
        );
    }
    Ok(())
}

pub(super) fn resolve_explicit_image(explicit: Option<&Path>, repo_dir: &Path) -> Result<PathBuf> {
    let path = explicit.ok_or_else(|| anyhow!("explicit image path was not provided"))?;
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_dir.join(path)
    })
}

pub(super) fn build_firmware_image(
    logger: &mut Logger,
    profile: &str,
    flash_log: &Path,
    repo_dir: &Path,
) -> Result<PathBuf> {
    let build = build_firmware_command(profile, repo_dir)?;
    logger.info(format!("building firmware ({profile}) before flash"));
    let status = run_command_logged(
        &build,
        flash_log,
        CommandRunOptions::new(DEFAULT_LOG_DRAIN_TIMEOUT),
    )?;
    if !status.success() {
        bail!("cargo build failed; see {}", flash_log.display());
    }

    Ok(repo_dir.join(format!("target/xtensa-esp32-none-elf/{profile}/meditamer")))
}

pub fn build_firmware_command(profile: &str, repo_dir: &Path) -> Result<CommandSpec> {
    match profile {
        "debug" | "release" | "ble-release" => {}
        _ => bail!("unsupported profile `{profile}` (use debug|release|ble-release)"),
    }

    let script = repo_dir.join("scripts/build/build.sh");
    if !script.is_file() {
        bail!("missing firmware build script {}", script.display());
    }

    Ok(CommandSpec::new(script.as_os_str())
        .arg(profile)
        .current_dir(repo_dir)
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS"))
}
