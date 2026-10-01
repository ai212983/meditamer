use std::net::{IpAddr, SocketAddr};

use anyhow::{anyhow, Result};

use super::report::PsramSample;
use super::MOUNTAIN_PACK_BYTES;

fn timing_field(line: &str, key: &str) -> Option<u64> {
    line.split_whitespace().find_map(|field| {
        let (field_key, value) = field.split_once('=')?;
        (field_key == key).then(|| value.parse().ok()).flatten()
    })
}

pub(super) fn verify_resident_timing(line: &str) -> Result<()> {
    if !line.starts_with("MOUNTAIN_TIMING ")
        || !line.split_whitespace().any(|field| field == "ok=true")
        || timing_field(line, "resident_bytes") != Some(MOUNTAIN_PACK_BYTES)
        || timing_field(line, "histogram_reads") != Some(0)
        || timing_field(line, "render_reads") != Some(0)
        || timing_field(line, "total_us").is_none_or(|value| value == 0)
    {
        return Err(anyhow!("Mountain resident timing contract failed: {line}"));
    }
    Ok(())
}

pub(super) fn mountain_ui_status_values(line: &str) -> Option<(bool, bool, u32)> {
    let mut fields = line.strip_prefix("MOUNTAIN_STATUS ")?.split_whitespace();
    let adopted = fields.next()?.strip_prefix("adopted=")?;
    let composed = fields.next()?.strip_prefix("composed=")?;
    let tx_drop = fields.next()?.strip_prefix("tx_drop=")?.parse().ok()?;
    fields
        .next()
        .is_none()
        .then_some((adopted == "1", composed == "1", tx_drop))
}

pub(super) fn verify_cycle_psram(sample: &PsramSample) -> Result<()> {
    for key in [
        "external_free_bytes",
        "internal_free_bytes",
        "min_external_free_bytes",
        "min_internal_free_bytes",
    ] {
        if sample
            .fields
            .get(key)
            .and_then(|value| value.parse::<u64>().ok())
            == Some(0)
            || sample
                .fields
                .get(key)
                .and_then(|value| value.parse::<u64>().ok())
                .is_none()
        {
            return Err(anyhow!(
                "PSRAM snapshot `{}` has no {key}: {}",
                sample.label,
                sample.raw_line
            ));
        }
    }
    if sample.fields.get("large_alloc_fail").map(String::as_str) != Some("0") {
        return Err(anyhow!(
            "PSRAM snapshot `{}` reports allocation failures: {}",
            sample.label,
            sample.raw_line
        ));
    }
    Ok(())
}

/// Trailing `headroom=<n>` value of a `stack_diag` / `touch_core_stack_diag`
/// line. Pure so the sampling action and host tests share the parse.
pub(super) fn headroom_minimum(line: &str) -> Option<String> {
    line.rsplit("headroom=")
        .next()?
        .trim()
        .parse::<u32>()
        .ok()
        .map(|parsed| parsed.to_string())
}

/// Correlated `STACKSTATUS` reply, unlike lossy `stack_diag` log lines.
pub(super) fn stack_status_values(line: &str) -> Option<(u32, u32, u32)> {
    let mut fields = line.strip_prefix("STACK_STATUS ")?.split_whitespace();
    let cpu0 = fields.next()?.strip_prefix("cpu0=")?.parse().ok()?;
    let touch = fields.next()?.strip_prefix("touch=")?.parse().ok()?;
    let tx_drop = fields.next()?.strip_prefix("tx_drop=")?.parse().ok()?;
    fields.next().is_none().then_some((cpu0, touch, tx_drop))
}

/// Freshest `listening on <ip>:<port>` socket address in a console line.
/// Pure so the liveness action and host tests share the parse.
pub(super) fn snooped_listener(line: &str) -> Option<std::net::SocketAddr> {
    let (_, tail) = line.split_once("listening on ")?;
    let token = tail.split_whitespace().next()?;
    token.parse().ok()
}

/// Fall back to the live network status when the one-time listener message
/// was lost in a busy serial stream. The port is the upload product contract.
pub(super) fn metrics_listener(line: &str, port: u16, allow_closed: bool) -> Option<SocketAddr> {
    let fields = line.strip_prefix("METRICS NET ")?;
    let mut connected = false;
    let mut listening = false;
    let mut ip = None;
    for field in fields.split_whitespace() {
        connected |= field == "wifi_connected=1";
        listening |= field == "http_listening=1";
        if let Some(value) = field.strip_prefix("ip=") {
            ip = value.parse::<IpAddr>().ok();
        }
    }
    let ip = ip?;
    (connected && (listening || allow_closed) && !ip.is_unspecified())
        .then_some(SocketAddr::new(ip, port))
}
