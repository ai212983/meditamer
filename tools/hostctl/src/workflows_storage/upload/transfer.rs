use std::{fs, path::Path};

use anyhow::{Context, Result};
use reqwest::{blocking::Client, Method};
use urlencoding::encode;

use crate::env_utils;

use super::client::{request_raw, request_sd_busy_aware, RequestContext};

pub(super) fn mkdir_p(client: &Client, request_ctx: RequestContext<'_>, path: &str) -> Result<()> {
    let mut current = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        current.push('/');
        current.push_str(part);
        let url = format!(
            "http://{}:{}/mkdir?path={}",
            request_ctx.host,
            request_ctx.port,
            encode(&current).replace("%2F", "/")
        );
        let _ = request_sd_busy_aware(client, Method::POST, &url, Some(Vec::new()), request_ctx)?;
    }
    Ok(())
}

pub(super) fn rm_path(client: &Client, request_ctx: RequestContext<'_>, path: &str) -> Result<()> {
    let url = format!(
        "http://{}:{}/rm?path={}",
        request_ctx.host,
        request_ctx.port,
        encode(path).replace("%2F", "/")
    );
    let _ = request_sd_busy_aware(client, Method::DELETE, &url, Some(Vec::new()), request_ctx)?;
    Ok(())
}

pub(super) fn stat_path(
    client: &Client,
    request_ctx: RequestContext<'_>,
    path: &str,
) -> Result<()> {
    let url = format!(
        "http://{}:{}/stat?path={}",
        request_ctx.host,
        request_ctx.port,
        encode(path).replace("%2F", "/")
    );
    let _ = request_sd_busy_aware(client, Method::GET, &url, None, request_ctx)?;
    Ok(())
}

pub(super) fn upload_file(
    client: &Client,
    request_ctx: RequestContext<'_>,
    local_path: &Path,
    remote_path: &str,
) -> Result<()> {
    let data = fs::read(local_path)
        .with_context(|| format!("failed reading upload file {}", local_path.display()))?;

    let upload_url = format!(
        "http://{}:{}/upload?path={}",
        request_ctx.host,
        request_ctx.port,
        encode(remote_path).replace("%2F", "/")
    );
    let put_result = request_sd_busy_aware(
        client,
        Method::PUT,
        &upload_url,
        Some(data.clone()),
        request_ctx,
    );
    if put_result.is_ok() {
        return Ok(());
    }

    let abort_url = format!(
        "http://{}:{}/upload_abort",
        request_ctx.host, request_ctx.port
    );
    let _ = request_raw(
        client,
        Method::POST,
        &abort_url,
        Some(Vec::new()),
        request_ctx.token,
        request_ctx.timeout_sec,
    );

    let begin_url = format!(
        "http://{}:{}/upload_begin?path={}&size={}",
        request_ctx.host,
        request_ctx.port,
        encode(remote_path).replace("%2F", "/"),
        data.len()
    );
    let _ = request_sd_busy_aware(
        client,
        Method::POST,
        &begin_url,
        Some(Vec::new()),
        request_ctx,
    )?;

    // Keep fallback /upload_chunk requests coarse-grained to reduce per-request
    // HTTP and SD roundtrip overhead on constrained Wi-Fi links.
    let chunk_size = env_utils::parse_env_u64("HOSTCTL_UPLOAD_CHUNK_SIZE", 65536)? as usize;
    for chunk in data.chunks(chunk_size.max(1)) {
        let chunk_url = format!(
            "http://{}:{}/upload_chunk",
            request_ctx.host, request_ctx.port
        );
        let _ = request_sd_busy_aware(
            client,
            Method::PUT,
            &chunk_url,
            Some(chunk.to_vec()),
            request_ctx,
        )?;
    }

    let commit_url = format!(
        "http://{}:{}/upload_commit",
        request_ctx.host, request_ctx.port
    );
    if let Err(err) = request_sd_busy_aware(
        client,
        Method::POST,
        &commit_url,
        Some(Vec::new()),
        request_ctx,
    ) {
        let _ = request_raw(
            client,
            Method::POST,
            &abort_url,
            Some(Vec::new()),
            request_ctx.token,
            request_ctx.timeout_sec,
        );
        return Err(err);
    }

    Ok(())
}
