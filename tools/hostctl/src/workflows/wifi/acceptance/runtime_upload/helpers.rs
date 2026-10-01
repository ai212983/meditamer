use anyhow::Result;

use crate::workflows::{upload, wifi::common::PanicSignal};

pub(super) fn resolve_net_upload_retry_policy() -> Result<upload::UploadRetryPolicy> {
    // Acceptance-specific retry budget: tighter than the general `upload`
    // defaults (net recovery 8s / sd-busy 30s vs 45s / 180s). Formerly env-tunable
    // via HOSTCTL_NET_UPLOAD_*; now fixed constants (hostctl-env-audit.md).
    Ok(upload::UploadRetryPolicy {
        sd_busy_total_retry_sec: 30.0f64.max(1.0),
        net_recovery_timeout_sec: 8.0f64.max(0.1),
        net_recovery_poll_sec: 0.8f64.max(0.05),
        net_recovery_consecutive_health_successes: 2u32,
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

pub(super) fn refresh_upload_client_on_failure_enabled() -> bool {
    // Acceptance-internal knob (HOSTCTL_NET_UPLOAD_REFRESH_ON_FAILURE): on.
    true
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
