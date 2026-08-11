use super::flash::build_app_binary;
use super::{FlashCaptureOptions, OutputPaths};
use std::{
    env,
    fs::{self},
    path::Path,
    process::Command,
};

use anyhow::{anyhow, bail, Result};

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

pub fn archive_firmware_artifacts(
    image_path: &Path,
    outputs: &OutputPaths,
    repo_dir: &Path,
    profile: &str,
    skip_update_check: bool,
) -> Result<()> {
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

    let mut hash_lines = Vec::new();
    if outputs.firmware_elf.is_file() {
        hash_lines.push(format!(
            "{}  firmware.elf",
            sha256_file(&outputs.firmware_elf)?
        ));
    }
    hash_lines.push(format!("{}  app.bin", sha256_file(&outputs.app_bin)?));
    fs::write(&outputs.hashes, format!("{}\n", hash_lines.join("\n")))?;

    let git_head = command_stdout(repo_dir, "git", &["rev-parse", "HEAD"])
        .unwrap_or_else(|error| format!("unavailable: {error}"));
    let git_status = command_stdout(repo_dir, "git", &["status", "--porcelain=v1"])
        .unwrap_or_else(|error| format!("unavailable: {error}"));
    let features = env::var("CARGO_FEATURES").unwrap_or_default();
    fs::write(
        &outputs.build_metadata,
        format!(
            "profile={profile}\nfeatures={features}\nsource_image={}\ngit_head={}\ngit_status_begin\n{}\ngit_status_end\n",
            image_path.display(),
            git_head.trim(),
            git_status.trim_end()
        ),
    )?;
    Ok(())
}

fn copy_if_distinct(source: &Path, destination: &Path) -> Result<()> {
    if source == destination {
        return Ok(());
    }
    fs::copy(source, destination)?;
    Ok(())
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
