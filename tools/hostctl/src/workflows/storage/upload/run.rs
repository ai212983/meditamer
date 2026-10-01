use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use reqwest::Method;

use crate::{logging::Logger, workflows::common::repo_path};

use super::{
    client::{health_timeout_s, make_client, request_raw, RequestContext},
    pathing::{remote_join, walkdir_sorted},
    transfer::{mkdir_p, rm_path, stat_path, upload_file},
    UploadOptions, UploadRetryPolicy,
};

#[derive(Clone, Copy)]
struct UploadRunTarget<'a> {
    request_ctx: RequestContext<'a>,
    dst_root: &'a str,
}

#[derive(Clone, Copy)]
pub struct DirectUploadOptions<'a> {
    pub host: &'a str,
    pub port: u16,
    pub timeout_sec: f64,
    pub src: &'a Path,
    pub dst_root: &'a str,
    pub token: Option<&'a str>,
    pub retry_policy: UploadRetryPolicy,
}

pub fn run_upload(logger: &mut Logger, mut opts: UploadOptions) -> Result<()> {
    opts.src = opts.src.map(repo_path);
    let client = make_client(opts.timeout_sec)?;
    let retry_policy = general_retry_policy();
    let token = opts.token.as_deref();
    let request_ctx = RequestContext {
        host: &opts.host,
        port: opts.port,
        timeout_sec: opts.timeout_sec,
        token,
        retry_policy,
    };

    if opts.src.is_none() && opts.rm.is_empty() {
        return Err(anyhow!("Nothing to do: provide --src and/or --rm"));
    }

    require_health_check(&client, &opts.host, opts.port, opts.timeout_sec, 20, 300)?;

    for rm in &opts.rm {
        let remote = if rm.starts_with('/') {
            rm.clone()
        } else {
            remote_join(&opts.dst, Path::new(rm))
        };
        logger.info(format!("[delete] {remote}"));
        rm_path(&client, request_ctx, &remote)?;
    }

    let Some(src) = opts.src else {
        logger.info("Delete complete.");
        return Ok(());
    };

    if !src.exists() {
        return Err(anyhow!("Source path does not exist: {}", src.display()));
    }

    let upload_target = UploadRunTarget {
        request_ctx,
        dst_root: &opts.dst,
    };

    if src.is_file() {
        return run_single_file_upload(logger, &client, upload_target, &src);
    }

    run_directory_upload(logger, &client, upload_target, &src)
}

fn run_single_file_upload(
    logger: &mut Logger,
    client: &reqwest::blocking::Client,
    target: UploadRunTarget<'_>,
    src: &Path,
) -> Result<()> {
    let remote_file = remote_join(
        target.dst_root,
        Path::new(src.file_name().unwrap_or_default()),
    );
    let remote_dir = Path::new(&remote_file)
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/".to_string());

    logger.info(format!("[mkdir -p] {remote_dir}"));
    mkdir_p(client, target.request_ctx, &remote_dir)?;

    logger.info(format!("[upload] {} -> {remote_file}", src.display()));
    upload_file(client, target.request_ctx, src, &remote_file)?;

    logger.info("Upload complete.");
    Ok(())
}

fn run_directory_upload(
    logger: &mut Logger,
    client: &reqwest::blocking::Client,
    target: UploadRunTarget<'_>,
    src: &Path,
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
        let remote_dir = remote_join(target.dst_root, &rel_dir);
        logger.info(format!("[mkdir -p] {remote_dir}"));
        mkdir_p(client, target.request_ctx, &remote_dir)?;
    }

    for (rel_file, local_file) in files {
        let remote_file = remote_join(target.dst_root, &rel_file);
        logger.info(format!(
            "[upload] {} -> {remote_file}",
            local_file.display()
        ));
        upload_file(client, target.request_ctx, &local_file, &remote_file)?;
    }

    logger.info("Upload complete.");
    Ok(())
}

pub fn make_direct_upload_client(timeout_sec: f64) -> Result<reqwest::blocking::Client> {
    make_client(timeout_sec)
}

pub fn upload_file_direct_fast_with_client(
    logger: &mut Logger,
    client: &reqwest::blocking::Client,
    opts: DirectUploadOptions<'_>,
) -> Result<()> {
    if !opts.src.exists() {
        return Err(anyhow!(
            "Source path does not exist: {}",
            opts.src.display()
        ));
    }

    // Keep wifi-acceptance failures bounded when host->device HTTP path is broken.
    require_health_check(client, opts.host, opts.port, opts.timeout_sec, 3, 250)?;
    let upload_target = UploadRunTarget {
        request_ctx: RequestContext {
            host: opts.host,
            port: opts.port,
            timeout_sec: opts.timeout_sec,
            token: opts.token,
            retry_policy: opts.retry_policy,
        },
        dst_root: opts.dst_root,
    };

    run_single_file_upload(logger, client, upload_target, opts.src)
}

pub fn stat_remote_file(
    host: &str,
    port: u16,
    timeout_sec: f64,
    remote_path: &str,
    token: Option<&str>,
    retry_policy: UploadRetryPolicy,
) -> Result<bool> {
    let client = make_client(timeout_sec)?;
    let request_ctx = RequestContext {
        host,
        port,
        timeout_sec,
        token,
        retry_policy,
    };
    match stat_path(&client, request_ctx, remote_path) {
        Ok(()) => Ok(true),
        Err(err) => {
            let msg = err.to_string().to_lowercase();
            if msg.contains("404") || msg.contains("not found") {
                Ok(false)
            } else {
                Err(err)
            }
        }
    }
}

fn general_retry_policy() -> UploadRetryPolicy {
    // General uploads retain their longer recovery window; acceptance uses
    // its own fixed, tighter budget in runtime_upload/helpers.rs.
    UploadRetryPolicy {
        sd_busy_total_retry_sec: 180.0,
        net_recovery_timeout_sec: 45.0,
        net_recovery_poll_sec: 0.8,
        net_recovery_consecutive_health_successes: 2,
    }
}

fn require_health_check(
    client: &reqwest::blocking::Client,
    host: &str,
    port: u16,
    timeout_sec: f64,
    attempts: u32,
    retry_delay_ms: u64,
) -> Result<()> {
    let attempt_count = attempts.max(1);
    let health_url = format!("http://{host}:{port}/health");
    let mut last_error = String::from("<none>");
    for idx in 0..attempt_count {
        match request_raw(
            client,
            Method::GET,
            &health_url,
            None,
            None,
            health_timeout_s(timeout_sec),
        ) {
            Ok(_) => return Ok(()),
            Err(err) => {
                last_error = format!("{err:#}");
            }
        }
        if idx + 1 < attempt_count {
            thread::sleep(Duration::from_millis(retry_delay_ms));
        }
    }
    Err(anyhow!(
        "health check failed: GET {health_url} (attempts={attempt_count}) last_error={last_error}"
    ))
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
