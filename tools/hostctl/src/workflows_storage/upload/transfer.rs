use std::{fs, path::Path};

use anyhow::{Context, Result};
use reqwest::{blocking::Client, Method};
use urlencoding::encode;

use crate::env_utils;

use super::client::{request_raw, request_sd_busy_aware};

pub(super) fn mkdir_p(
    client: &Client,
    host: &str,
    port: u16,
    timeout_sec: f64,
    path: &str,
    token: Option<&str>,
) -> Result<()> {
    let mut current = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        current.push('/');
        current.push_str(part);
        let url = format!(
            "http://{host}:{port}/mkdir?path={}",
            encode(&current).replace("%2F", "/")
        );
        let _ = request_sd_busy_aware(
            client,
            Method::POST,
            &url,
            Some(Vec::new()),
            token,
            host,
            port,
            timeout_sec,
        )?;
    }
    Ok(())
}

pub(super) fn rm_path(
    client: &Client,
    host: &str,
    port: u16,
    timeout_sec: f64,
    path: &str,
    token: Option<&str>,
) -> Result<()> {
    let url = format!(
        "http://{host}:{port}/rm?path={}",
        encode(path).replace("%2F", "/")
    );
    let _ = request_sd_busy_aware(
        client,
        Method::DELETE,
        &url,
        Some(Vec::new()),
        token,
        host,
        port,
        timeout_sec,
    )?;
    Ok(())
}

pub(super) fn upload_file(
    client: &Client,
    host: &str,
    port: u16,
    timeout_sec: f64,
    local_path: &Path,
    remote_path: &str,
    token: Option<&str>,
) -> Result<()> {
    let data = fs::read(local_path)
        .with_context(|| format!("failed reading upload file {}", local_path.display()))?;

    let upload_url = format!(
        "http://{host}:{port}/upload?path={}",
        encode(remote_path).replace("%2F", "/")
    );
    let put_result = request_sd_busy_aware(
        client,
        Method::PUT,
        &upload_url,
        Some(data.clone()),
        token,
        host,
        port,
        timeout_sec,
    );
    if put_result.is_ok() {
        return Ok(());
    }

    let abort_url = format!("http://{host}:{port}/upload_abort");
    let _ = request_raw(
        client,
        Method::POST,
        &abort_url,
        Some(Vec::new()),
        token,
        timeout_sec,
    );

    let begin_url = format!(
        "http://{host}:{port}/upload_begin?path={}&size={}",
        encode(remote_path).replace("%2F", "/"),
        data.len()
    );
    let _ = request_sd_busy_aware(
        client,
        Method::POST,
        &begin_url,
        Some(Vec::new()),
        token,
        host,
        port,
        timeout_sec,
    )?;

    // Keep fallback /upload_chunk requests coarse-grained to reduce per-request
    // HTTP and SD roundtrip overhead on constrained Wi-Fi links.
    let chunk_size = env_utils::parse_env_u64("HOSTCTL_UPLOAD_CHUNK_SIZE", 49152)? as usize;
    for chunk in data.chunks(chunk_size.max(1)) {
        let chunk_url = format!("http://{host}:{port}/upload_chunk");
        let _ = request_sd_busy_aware(
            client,
            Method::PUT,
            &chunk_url,
            Some(chunk.to_vec()),
            token,
            host,
            port,
            timeout_sec,
        )?;
    }

    let commit_url = format!("http://{host}:{port}/upload_commit");
    if let Err(err) = request_sd_busy_aware(
        client,
        Method::POST,
        &commit_url,
        Some(Vec::new()),
        token,
        host,
        port,
        timeout_sec,
    ) {
        let _ = request_raw(
            client,
            Method::POST,
            &abort_url,
            Some(Vec::new()),
            token,
            timeout_sec,
        );
        return Err(err);
    }

    Ok(())
}
