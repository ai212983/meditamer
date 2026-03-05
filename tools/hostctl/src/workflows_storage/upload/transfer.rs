use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use reqwest::{blocking::Client, Method};
use urlencoding::encode;

use crate::env_utils;

use super::client::{
    is_transport_reset_chunk_fallback_error, make_client, request_raw, request_sd_busy_aware,
    request_sd_busy_aware_timed, RequestContext,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UploadTransportMode {
    Auto,
    Direct,
    Chunked,
}

fn upload_transport_mode_from_env() -> Result<UploadTransportMode> {
    match std::env::var("HOSTCTL_UPLOAD_MODE") {
        Ok(raw) => {
            let normalized = raw.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "" | "auto" => Ok(UploadTransportMode::Auto),
                "direct" => Ok(UploadTransportMode::Direct),
                "chunked" => Ok(UploadTransportMode::Chunked),
                _ => Err(anyhow!(
                    "HOSTCTL_UPLOAD_MODE must be one of: auto, direct, chunked"
                )),
            }
        }
        Err(std::env::VarError::NotPresent) => Ok(UploadTransportMode::Auto),
        Err(err) => Err(anyhow!("HOSTCTL_UPLOAD_MODE invalid: {err}")),
    }
}

fn upload_send_diag_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_SEND_DIAG", false)
}

fn upload_fresh_client_per_upload_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_FRESH_CLIENT_PER_UPLOAD", false)
}

fn host_diag_log_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("HOSTCTL_UPLOAD_SEND_DIAG_PATH") {
        if !path.trim().is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    std::env::var("HOSTCTL_NET_LOG_PATH")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(|path| PathBuf::from(format!("{path}.hostdiag")))
}

fn append_host_diag_line(line: &str) {
    if let Some(path) = host_diag_log_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

fn log_direct_upload_diag(
    timed: &super::client::TimedResponse,
    remote_path: &str,
    request_ctx: RequestContext<'_>,
) {
    let cadence = timed.timing.body_cadence;
    let bytes_per_read = if cadence.read_calls == 0 {
        0
    } else {
        cadence.read_bytes / cadence.read_calls as usize
    };
    let line = format!(
        "host_upload_send_diag: mode=direct path={} target={}:{} attempts={} body_bytes={} send_ms={} resp_read_ms={} total_ms={} body_read_calls={} body_short_reads={} body_bytes_per_read={} body_gap_ms_total={} body_gap_ms_max={} body_gap_over_10ms={} body_gap_over_50ms={}",
        remote_path,
        request_ctx.host,
        request_ctx.port,
        timed.attempts,
        timed.timing.body_bytes,
        timed.timing.send_ms,
        timed.timing.response_read_ms,
        timed.timing.total_ms,
        cadence.read_calls,
        cadence.short_reads,
        bytes_per_read,
        cadence.read_gap_ms_total,
        cadence.read_gap_ms_max,
        cadence.read_gap_over_10ms,
        cadence.read_gap_over_50ms,
    );
    println!("{line}");
    append_host_diag_line(&line);
}

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
    let mode = upload_transport_mode_from_env()?;
    let upload_client: Option<Client> = if upload_fresh_client_per_upload_enabled()? {
        Some(make_client(request_ctx.timeout_sec)?)
    } else {
        None
    };
    let request_client: &Client = upload_client.as_ref().unwrap_or(client);

    if mode != UploadTransportMode::Chunked {
        let upload_url = format!(
            "http://{}:{}/upload?path={}",
            request_ctx.host,
            request_ctx.port,
            encode(remote_path).replace("%2F", "/")
        );
        let send_diag = upload_send_diag_enabled()?;
        let put_err = if send_diag {
            match request_sd_busy_aware_timed(
                request_client,
                Method::PUT,
                &upload_url,
                Some(data.clone()),
                request_ctx,
            ) {
                Ok(timed) => {
                    log_direct_upload_diag(&timed, remote_path, request_ctx);
                    return Ok(());
                }
                Err(err) => err,
            }
        } else {
            match request_sd_busy_aware(
                request_client,
                Method::PUT,
                &upload_url,
                Some(data.clone()),
                request_ctx,
            ) {
                Ok(_) => return Ok(()),
                Err(err) => err,
            }
        };
        if mode == UploadTransportMode::Direct {
            if is_transport_reset_chunk_fallback_error(&put_err) {
                let line = format!(
                    "host_upload_transport_fallback: mode=direct reason=transport_reset_streak path={remote_path} fresh_client=1"
                );
                println!("{line}");
                append_host_diag_line(&line);
                let fallback_client = make_client(request_ctx.timeout_sec)?;
                return upload_file_chunked(
                    &fallback_client,
                    request_ctx,
                    &data,
                    remote_path,
                    true,
                );
            }
            return Err(put_err);
        }
        return upload_file_chunked(request_client, request_ctx, &data, remote_path, true);
    }

    upload_file_chunked(request_client, request_ctx, &data, remote_path, false)
}

fn upload_file_chunked(
    client: &Client,
    request_ctx: RequestContext<'_>,
    data: &[u8],
    remote_path: &str,
    force_abort_before_begin: bool,
) -> Result<()> {
    let abort_url = format!(
        "http://{}:{}/upload_abort",
        request_ctx.host, request_ctx.port
    );
    if force_abort_before_begin {
        let _ = request_raw(
            client,
            Method::POST,
            &abort_url,
            Some(Vec::new()),
            request_ctx.token,
            request_ctx.timeout_sec,
        );
    }

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

#[cfg(test)]
mod tests;
