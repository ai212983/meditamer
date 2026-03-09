use crate::firmware::{
    storage::upload::wifi::{
        wifi_active_scan_config, wifi_channel_active_scan_config, wifi_directed_active_scan_config,
        wifi_passive_scan_config, wifi_raw_broad_scan_config, ScanConfig,
    },
    types::WifiRuntimePolicy,
};

const WIFI_SCAN_RESULT_MAX_DEFAULT: usize = 64;
const WIFI_SCAN_RESULT_MAX_MIN: usize = 4;
const WIFI_SCAN_RESULT_MAX: usize = {
    let configured = match option_env!("MEDITAMER_WIFI_SCAN_RESULT_MAX") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SCAN_RESULT_MAX"),
    };
    match configured {
        Some(raw) => match parse_ascii_usize(raw) {
            Some(value)
                if value >= WIFI_SCAN_RESULT_MAX_MIN && value <= WIFI_SCAN_RESULT_MAX_DEFAULT =>
            {
                value
            }
            _ => WIFI_SCAN_RESULT_MAX_DEFAULT,
        },
        None => WIFI_SCAN_RESULT_MAX_DEFAULT,
    }
};

const fn parse_ascii_usize(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut idx = 0;
    let mut parsed = 0usize;
    while idx < bytes.len() {
        let ch = bytes[idx];
        if ch < b'0' || ch > b'9' {
            return None;
        }
        parsed = parsed
            .saturating_mul(10)
            .saturating_add((ch - b'0') as usize);
        idx += 1;
    }
    Some(parsed)
}

pub(super) fn active_scan_config(policy: WifiRuntimePolicy) -> ScanConfig<'static> {
    wifi_active_scan_config(
        WIFI_SCAN_RESULT_MAX,
        policy.scan_active_min_ms as u64,
        policy.scan_active_max_ms as u64,
    )
}

pub(super) fn directed_active_scan_config(
    target_ssid: &str,
    policy: WifiRuntimePolicy,
) -> ScanConfig<'_> {
    wifi_directed_active_scan_config(
        target_ssid,
        WIFI_SCAN_RESULT_MAX,
        policy.scan_active_min_ms as u64,
        policy.scan_active_max_ms as u64,
    )
}

pub(super) fn channel_active_scan_config(
    channel: u8,
    policy: WifiRuntimePolicy,
) -> ScanConfig<'static> {
    wifi_channel_active_scan_config(
        channel,
        WIFI_SCAN_RESULT_MAX,
        policy.scan_active_min_ms as u64,
        policy.scan_active_max_ms as u64,
    )
}

pub(super) fn passive_scan_config(policy: WifiRuntimePolicy) -> ScanConfig<'static> {
    wifi_passive_scan_config(WIFI_SCAN_RESULT_MAX, policy.scan_passive_ms as u64)
}

pub(super) fn raw_broad_scan_config() -> ScanConfig<'static> {
    wifi_raw_broad_scan_config(WIFI_SCAN_RESULT_MAX)
}
