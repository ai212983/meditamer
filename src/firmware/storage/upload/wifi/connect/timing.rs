use super::*;
pub(crate) fn active_scan_timeout_ms(policy: WifiRuntimePolicy) -> u64 {
    // Directed active scan timeout should be shorter than passive all-channel
    // scans but still high enough to tolerate noisy RF conditions.
    // `scan_active_{min,max}` are per-channel dwell values; we multiply by
    // expected multi-channel sweep overhead and clamp to a bounded window so
    // one round cannot consume the full recovery budget.
    // Source (esp-radio ScanTypeConfig docs): https://docs.espressif.com/projects/rust/esp-radio/0.16.0/esp32s3/src/esp_radio/wifi/mod.rs.html
    (policy.scan_active_max_ms.max(policy.scan_active_min_ms) as u64)
        .saturating_mul(10)
        .clamp(8_000, 25_000)
}

pub(crate) fn directed_scan_timeout_ms(policy: WifiRuntimePolicy) -> u64 {
    active_scan_timeout_ms(policy).clamp(3_000, 8_000)
}

pub(crate) fn passive_scan_timeout_ms(policy: WifiRuntimePolicy) -> u64 {
    // Passive scanning walks all channels; timeout must scale with per-channel
    // dwell. A short fixed timeout causes false "zero discovery" even when APs exist.
    // The 16x factor and +3s guard absorb channel-switch and driver scheduling
    // overhead seen in field traces while keeping total round time bounded.
    // Source (per-channel passive dwell + 1500ms caution): https://docs.espressif.com/projects/rust/esp-radio/0.16.0/esp32s3/src/esp_radio/wifi/mod.rs.html
    (policy.scan_passive_ms as u64)
        .saturating_mul(16)
        .saturating_add(3_000)
        .clamp(15_000, 90_000)
}

pub(crate) fn zero_discovery_probe_timeout_ms(policy: WifiRuntimePolicy) -> u64 {
    active_scan_timeout_ms(policy).clamp(2_500, 6_000)
}

pub(crate) fn zero_discovery_probe_budget_ms(policy: WifiRuntimePolicy, full_channel: bool) -> u64 {
    let probe_channels = if full_channel {
        WIFI_CHANNEL_PROBE_SEQUENCE.len() as u64
    } else {
        WIFI_ZERO_DISCOVERY_SCAN_PROBE_CHANNELS.len() as u64
    };
    zero_discovery_probe_timeout_ms(policy).saturating_mul(probe_channels)
}

pub(crate) fn post_recover_watchdog_timeout_ms(policy: WifiRuntimePolicy) -> u64 {
    // Watchdog budget intentionally covers at least one full discovery cycle
    // (active + directed + passive + zero-result probes + reconnect overhead)
    // to avoid resetting recovery state before channel/auth rotation can progress.
    // Keep this larger than one connect timeout so the connect API one-shot
    // semantics can be retried through the recovery ladder without premature reset.
    // Source (`esp_wifi_connect` single-attempt behavior): https://docs.espressif.com/projects/esp-idf/en/v5.3.1/esp32/api-reference/network/esp_wifi.html#_CPPv416esp_wifi_connectv
    let scan_budget_ms = active_scan_timeout_ms(policy)
        .saturating_add(directed_scan_timeout_ms(policy))
        .saturating_add(passive_scan_timeout_ms(policy))
        // Budget for forced full-channel probe path; this prevents watchdog
        // reset from preempting the hard-guard scan escalation.
        .saturating_add(zero_discovery_probe_budget_ms(policy, true))
        .saturating_add(6_000);
    (policy.connect_timeout_ms as u64)
        .saturating_add(scan_budget_ms)
        .max((policy.connect_timeout_ms as u64).saturating_mul(2))
}
