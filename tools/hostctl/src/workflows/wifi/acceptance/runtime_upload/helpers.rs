use anyhow::Result;

use crate::{
    env_utils,
    workflows::{upload, wifi::common::PanicSignal},
};

pub(super) fn resolve_net_upload_retry_policy() -> Result<upload::UploadRetryPolicy> {
    let default_sd_busy_retry_s =
        env_utils::parse_env_f64("HOSTCTL_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC", 30.0)?.max(1.0);
    let default_net_recovery_timeout_s =
        env_utils::parse_env_f64("HOSTCTL_UPLOAD_NET_RECOVERY_TIMEOUT_SEC", 8.0)?.max(0.1);
    let default_net_recovery_poll_s =
        env_utils::parse_env_f64("HOSTCTL_UPLOAD_NET_RECOVERY_POLL_SEC", 0.8)?.max(0.05);
    let default_net_recovery_consecutive_health =
        env_utils::parse_env_u32("HOSTCTL_UPLOAD_NET_RECOVERY_CONSECUTIVE_HEALTH", 2)?.max(1);
    Ok(upload::UploadRetryPolicy {
        sd_busy_total_retry_sec: env_utils::parse_env_f64(
            "HOSTCTL_NET_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC",
            default_sd_busy_retry_s,
        )?
        .max(1.0),
        net_recovery_timeout_sec: env_utils::parse_env_f64(
            "HOSTCTL_NET_UPLOAD_NET_RECOVERY_TIMEOUT_SEC",
            default_net_recovery_timeout_s,
        )?
        .max(0.1),
        net_recovery_poll_sec: env_utils::parse_env_f64(
            "HOSTCTL_NET_UPLOAD_NET_RECOVERY_POLL_SEC",
            default_net_recovery_poll_s,
        )?
        .max(0.05),
        net_recovery_consecutive_health_successes: env_utils::parse_env_u32(
            "HOSTCTL_NET_UPLOAD_NET_RECOVERY_CONSECUTIVE_HEALTH",
            default_net_recovery_consecutive_health,
        )?
        .max(1),
    })
}

pub(super) fn append_health_fail_net_status(
    detail: &mut String,
    status_query: std::result::Result<Option<String>, String>,
) -> String {
    match status_query {
        Ok(Some(line)) => {
            detail.push_str(&format!("; net_status={line}"));
            format!("health_fail_diag: {line}")
        }
        Ok(None) => {
            detail.push_str("; net_status=<unavailable>");
            "health_fail_diag: NET_STATUS unavailable".to_string()
        }
        Err(err) => {
            detail.push_str(&format!("; net_status_query_error={err}"));
            format!("health_fail_diag: NET_STATUS query failed ({err})")
        }
    }
}

pub(super) fn classify_host_upload_failure(detail: &str) -> Option<&'static str> {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("health check failed: get")
        && (lower.contains("send failed")
            || lower.contains("connection reset")
            || lower.contains("connection refused")
            || lower.contains("error sending request"))
    {
        return Some("host_health_send_fail");
    }
    if lower.contains("connection refused") {
        return Some("host_transport_connect_refused");
    }
    if lower.contains("send failed") || lower.contains("error sending request") {
        return Some("host_transport_send_fail");
    }
    if lower.contains("connection reset") || lower.contains("broken pipe") {
        return Some("host_transport_connection_reset");
    }
    None
}

pub(super) fn refresh_retry_eligible_host_failure(class: &str) -> bool {
    matches!(
        class,
        "host_health_send_fail" | "host_transport_send_fail" | "host_transport_connection_reset"
    )
}

pub(super) fn refresh_upload_client_on_failure_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_NET_UPLOAD_REFRESH_ON_FAILURE", true)
}

pub(super) fn append_panic_signal_context(
    detail: &mut String,
    signal: Option<&PanicSignal>,
) -> bool {
    let Some(signal) = signal else {
        return false;
    };
    detail.push_str(&format!(
        "; panic_class={} panic_line_index={} panic_line={}",
        signal.class.as_str(),
        signal.marker_index,
        signal.marker_line
    ));
    true
}

pub(super) fn avg(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

pub(super) fn parse_metrics_key_u32(line: &str, key: &str) -> Option<u32> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
        .and_then(|value| value.parse::<u32>().ok())
}
