use std::{
    fs,
    io::Write,
    path::PathBuf,
};

use anyhow::Result;
use reqwest::Method;

use crate::env_utils;

use super::{
    DIRECT_BURST_BYTES_DEFAULT, DIRECT_BURST_BYTES_MAX, DIRECT_BURST_BYTES_MIN,
    DIRECT_BURST_PRE_PUT_DELAY_MS_DEFAULT, TRANSPORT_RESET_CHUNK_FALLBACK_STREAK_DEFAULT,
    TRANSPORT_RESET_FAST_RETRY_STREAK_DEFAULT,
};

pub(super) fn upload_send_diag_enabled() -> bool {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_SEND_DIAG", false).unwrap_or(false)
}

pub(super) fn upload_send_diag_deep_enabled() -> bool {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_SEND_DIAG_DEEP", false).unwrap_or(false)
}

pub(super) fn upload_force_connection_close() -> bool {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_FORCE_CONN_CLOSE", false).unwrap_or(false)
}

pub(super) fn upload_direct_burst_sender_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_DIRECT_BURST_SENDER", false)
}

pub(super) fn upload_direct_burst_mode_active() -> bool {
    upload_direct_burst_sender_enabled().unwrap_or(false)
}

pub(super) fn upload_direct_burst_bytes() -> Result<usize> {
    let configured = env_utils::parse_env_u64(
        "HOSTCTL_UPLOAD_DIRECT_BURST_BYTES",
        DIRECT_BURST_BYTES_DEFAULT as u64,
    )? as usize;
    Ok(configured.clamp(DIRECT_BURST_BYTES_MIN, DIRECT_BURST_BYTES_MAX))
}

pub(super) fn upload_tcp_nodelay_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_TCP_NODELAY", true)
}

pub(super) fn upload_pre_put_delay_ms() -> Result<u64> {
    let default = if upload_direct_burst_mode_active() {
        DIRECT_BURST_PRE_PUT_DELAY_MS_DEFAULT
    } else {
        0
    };
    env_utils::parse_env_u64("HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS", default)
}

pub(super) fn upload_transport_reset_fast_retry_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY", true)
}

pub(super) fn upload_transport_reset_fast_retry_streak_limit() -> Result<u32> {
    Ok(env_utils::parse_env_u32(
        "HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY_STREAK",
        TRANSPORT_RESET_FAST_RETRY_STREAK_DEFAULT,
    )?
    .max(1))
}

pub(super) fn upload_transport_reset_chunk_fallback_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_TRANSPORT_RESET_CHUNK_FALLBACK", true)
}

pub(super) fn upload_transport_reset_chunk_fallback_streak_limit() -> Result<u32> {
    Ok(env_utils::parse_env_u32(
        "HOSTCTL_UPLOAD_TRANSPORT_RESET_CHUNK_FALLBACK_STREAK",
        TRANSPORT_RESET_CHUNK_FALLBACK_STREAK_DEFAULT,
    )?
    .max(1))
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

pub(super) fn append_host_diag_line(line: &str) {
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

pub(super) fn should_use_direct_burst_sender(method: &Method, url: &str) -> Result<bool> {
    if !upload_direct_burst_sender_enabled()? {
        return Ok(false);
    }
    Ok(*method == Method::PUT && url.contains("/upload?"))
}
