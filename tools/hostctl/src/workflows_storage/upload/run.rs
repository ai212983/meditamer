use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use reqwest::Method;

use crate::{env_utils, logging::Logger};

use super::{
    client::{health_timeout_s, make_client, request_raw},
    pathing::{remote_join, walkdir_sorted},
    transfer::{mkdir_p, rm_path, upload_file},
    UploadOptions,
};

pub fn run_upload(logger: &mut Logger, opts: UploadOptions) -> Result<()> {
    let client = make_client(opts.timeout_sec)?;

    if opts.src.is_none() && opts.rm.is_empty() {
        return Err(anyhow!("Nothing to do: provide --src and/or --rm"));
    }

    let health_url = format!("http://{}:{}/health", opts.host, opts.port);
    let mut health_ok = false;
    for _ in 0..20 {
        if request_raw(
            &client,
            Method::GET,
            &health_url,
            None,
            None,
            health_timeout_s(opts.timeout_sec),
        )
        .is_ok()
        {
            health_ok = true;
            break;
        }
        thread::sleep(Duration::from_millis(300));
    }
    if !health_ok {
        return Err(anyhow!("health check failed"));
    }

    for rm in &opts.rm {
        let remote = if rm.starts_with('/') {
            rm.clone()
        } else {
            remote_join(&opts.dst, Path::new(rm))
        };
        logger.info(format!("[delete] {remote}"));
        rm_path(
            &client,
            &opts.host,
            opts.port,
            opts.timeout_sec,
            &remote,
            opts.token.as_deref(),
        )?;
    }

    let Some(src) = opts.src else {
        logger.info("Delete complete.");
        return Ok(());
    };

    if !src.exists() {
        return Err(anyhow!("Source path does not exist: {}", src.display()));
    }

    let skip_mkdir = env_utils::parse_env_bool01("HOSTCTL_UPLOAD_SKIP_MKDIR", false)?;

    if src.is_file() {
        return run_single_file_upload(
            logger,
            &client,
            &opts.host,
            opts.port,
            opts.timeout_sec,
            &src,
            &opts.dst,
            opts.token.as_deref(),
            skip_mkdir,
        );
    }

    run_directory_upload(
        logger,
        &client,
        &opts.host,
        opts.port,
        opts.timeout_sec,
        &src,
        &opts.dst,
        opts.token.as_deref(),
        skip_mkdir,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_single_file_upload(
    logger: &mut Logger,
    client: &reqwest::blocking::Client,
    host: &str,
    port: u16,
    timeout_sec: f64,
    src: &Path,
    dst_root: &str,
    token: Option<&str>,
    skip_mkdir: bool,
) -> Result<()> {
    let remote_file = remote_join(dst_root, Path::new(src.file_name().unwrap_or_default()));
    let remote_dir = Path::new(&remote_file)
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/".to_string());

    if skip_mkdir {
        logger.info(format!("[mkdir -p] skipped ({remote_dir})"));
    } else {
        logger.info(format!("[mkdir -p] {remote_dir}"));
        mkdir_p(client, host, port, timeout_sec, &remote_dir, token)?;
    }

    logger.info(format!("[upload] {} -> {remote_file}", src.display()));
    upload_file(client, host, port, timeout_sec, src, &remote_file, token)?;

    logger.info("Upload complete.");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_directory_upload(
    logger: &mut Logger,
    client: &reqwest::blocking::Client,
    host: &str,
    port: u16,
    timeout_sec: f64,
    src: &Path,
    dst_root: &str,
    token: Option<&str>,
    skip_mkdir: bool,
) -> Result<()> {
    let mut dirs = vec![PathBuf::from(".")];
    let mut files = Vec::new();
    for entry in walkdir_sorted(src)? {
        if entry.is_dir() {
            let rel = entry
                .strip_prefix(src)
                .unwrap_or(entry.as_path())
                .to_path_buf();
            dirs.push(rel);
        } else if entry.is_file() {
            let rel = entry
                .strip_prefix(src)
                .unwrap_or(entry.as_path())
                .to_path_buf();
            files.push((rel, entry.to_path_buf()));
        }
    }

    dirs.sort();
    dirs.dedup();

    for rel_dir in dirs {
        let remote_dir = remote_join(dst_root, &rel_dir);
        if skip_mkdir {
            logger.info(format!("[mkdir -p] skipped ({remote_dir})"));
            continue;
        }
        logger.info(format!("[mkdir -p] {remote_dir}"));
        mkdir_p(client, host, port, timeout_sec, &remote_dir, token)?;
    }

    for (rel_file, local_file) in files {
        let remote_file = remote_join(dst_root, &rel_file);
        logger.info(format!(
            "[upload] {} -> {remote_file}",
            local_file.display()
        ));
        upload_file(
            client,
            host,
            port,
            timeout_sec,
            &local_file,
            &remote_file,
            token,
        )?;
    }

    logger.info("Upload complete.");
    Ok(())
}

pub fn upload_file_direct_fast(
    logger: &mut Logger,
    host: &str,
    port: u16,
    timeout_sec: f64,
    src: &Path,
    dst_root: &str,
    token: Option<&str>,
) -> Result<()> {
    if !src.exists() {
        return Err(anyhow!("Source path does not exist: {}", src.display()));
    }

    let client = make_client(timeout_sec)?;
    let skip_mkdir = env_utils::parse_env_bool01("HOSTCTL_UPLOAD_SKIP_MKDIR", false)?;

    run_single_file_upload(
        logger,
        &client,
        host,
        port,
        timeout_sec,
        src,
        dst_root,
        token,
        skip_mkdir,
    )
}
