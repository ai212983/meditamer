use super::flash::build_app_binary;
use super::{FlashCaptureOptions, OutputPaths};
use std::{
    env,
    fs::{self},
    path::Path,
    process::Command,
};

use anyhow::{anyhow, bail, Result};

const OTA_SLOT_BYTES: u64 = 0x1f0000;
const OTA_MIN_HEADROOM_BYTES: u64 = 0x20000;

pub(super) struct ArchiveFirmwareArtifactsOptions<'a> {
    pub(super) image_path: &'a Path,
    pub(super) outputs: &'a OutputPaths,
    pub(super) repo_dir: &'a Path,
    pub(super) profile: &'a str,
    pub(super) skip_update_check: bool,
    pub(super) include_bootloader: bool,
    pub(super) built_in_workflow: bool,
}

pub fn validate_post_command_options(opts: &FlashCaptureOptions) -> Result<()> {
    match (opts.post_command.as_deref(), opts.post_pattern.as_deref()) {
        (Some(command), Some(pattern)) if !command.trim().is_empty() && !pattern.is_empty() => {}
        (None, None) => return Ok(()),
        (Some(_), None) => bail!("--post-pattern is required with --post-command"),
        (None, Some(_)) => bail!("--post-command is required with --post-pattern"),
        _ => bail!("post command and pattern must not be empty"),
    }
    if opts.post_timeout_ms == Some(0) {
        bail!("--post-timeout-ms must be greater than zero");
    }
    Ok(())
}

pub(super) fn archive_firmware_artifacts(opts: ArchiveFirmwareArtifactsOptions<'_>) -> Result<()> {
    let ArchiveFirmwareArtifactsOptions {
        image_path,
        outputs,
        repo_dir,
        profile,
        skip_update_check,
        include_bootloader,
        built_in_workflow,
    } = opts;
    if !image_path.is_file() {
        bail!("firmware image does not exist: {}", image_path.display());
    }

    let source_is_bin = image_path.extension().and_then(|ext| ext.to_str()) == Some("bin");
    if source_is_bin {
        copy_if_distinct(image_path, &outputs.app_bin)?;
    } else {
        copy_if_distinct(image_path, &outputs.firmware_elf)?;
        let generated = build_app_binary(image_path, &outputs.flash_log, skip_update_check)?;
        copy_if_distinct(&generated, &outputs.app_bin)?;
    }
    validate_app_capacity(&outputs.app_bin)?;

    if include_bootloader {
        let built_bootloader = repo_dir.join("target/ota-bootloader/bootloader/bootloader.bin");
        let built_partition_table =
            repo_dir.join("target/ota-bootloader/partition_table/partition-table.bin");
        for path in [&built_bootloader, &built_partition_table] {
            if !path.is_file() {
                bail!("OTA build artifact does not exist: {}", path.display());
            }
        }
        copy_if_distinct(&built_bootloader, &outputs.bootloader_bin)?;
        copy_if_distinct(&built_partition_table, &outputs.partition_table_bin)?;
    } else {
        remove_stale_artifact(&outputs.bootloader_bin)?;
        remove_stale_artifact(&outputs.partition_table_bin)?;
    }

    let mut hash_lines = Vec::new();
    if outputs.firmware_elf.is_file() {
        hash_lines.push(format!(
            "{}  firmware.elf",
            sha256_file(&outputs.firmware_elf)?
        ));
    }
    hash_lines.push(format!("{}  app.bin", sha256_file(&outputs.app_bin)?));
    if include_bootloader {
        hash_lines.push(format!(
            "{}  bootloader.bin",
            sha256_file(&outputs.bootloader_bin)?
        ));
        hash_lines.push(format!(
            "{}  partition-table.bin",
            sha256_file(&outputs.partition_table_bin)?
        ));
    }
    fs::write(&outputs.hashes, format!("{}\n", hash_lines.join("\n")))?;

    let git_head = command_stdout(repo_dir, "git", &["rev-parse", "HEAD"])
        .unwrap_or_else(|error| format!("unavailable: {error}"));
    let git_status = command_stdout(repo_dir, "git", &["status", "--porcelain=v1"])
        .unwrap_or_else(|error| format!("unavailable: {error}"));
    let (requested_features, no_default_features) = if built_in_workflow && profile == "ble-release"
    {
        // The canonical build script rejects every other BLE release feature
        // configuration, so these fields describe the command that produced
        // the archived ELF rather than ambient host metadata.
        ("ble-foundation".to_owned(), "false".to_owned())
    } else if built_in_workflow {
        (
            env::var("CARGO_FEATURES").unwrap_or_default(),
            if env::var("CARGO_NO_DEFAULT_FEATURES").as_deref() == Ok("1") {
                "true".to_owned()
            } else {
                "false".to_owned()
            },
        )
    } else {
        ("unverified".to_owned(), "unverified".to_owned())
    };
    let firmware_build_id =
        env::var("MEDITAMER_FIRMWARE_BUILD_ID").unwrap_or_else(|_| "unlabeled".to_owned());
    let firmware_public_key =
        env::var("MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX").unwrap_or_else(|_| "missing".to_owned());
    fs::write(
        &outputs.build_metadata,
        format!(
            "profile={profile}\nimage_source={}\nrequested_features={requested_features}\nno_default_features={no_default_features}\nfirmware_build_id={firmware_build_id}\nfirmware_public_key_hex={firmware_public_key}\nsource_image={}\ngit_head={}\ngit_status_begin\n{}\ngit_status_end\n",
            if built_in_workflow { "build" } else { "explicit" },
            image_path.display(),
            git_head.trim(),
            git_status.trim_end()
        ),
    )?;
    Ok(())
}

fn validate_app_capacity(path: &Path) -> Result<()> {
    let len = fs::metadata(path)?.len();
    let maximum = OTA_SLOT_BYTES - OTA_MIN_HEADROOM_BYTES;
    if len > maximum {
        bail!(
            "application image is {len} bytes; accepted OTA capacity floor requires at most {maximum} bytes"
        );
    }
    Ok(())
}

fn copy_if_distinct(source: &Path, destination: &Path) -> Result<()> {
    if source == destination {
        return Ok(());
    }
    fs::copy(source, destination)?;
    Ok(())
}

fn remove_stale_artifact(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()?;
    if !output.status.success() {
        bail!("shasum failed for {}", path.display());
    }
    String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("shasum returned no digest for {}", path.display()))
}

fn command_stdout(repo_dir: &Path, program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo_dir)
        .output()?;
    if !output.status.success() {
        bail!("{program} {} failed", args.join(" "));
    }
    Ok(String::from_utf8(output.stdout)?)
}
