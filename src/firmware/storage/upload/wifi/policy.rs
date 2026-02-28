use crate::firmware::types::{WifiRuntimePolicy, WIFI_DHCP_TIMEOUT_MAX_MS};

pub(super) fn effective_dhcp_timeout_ms(
    runtime_policy: WifiRuntimePolicy,
    pinned_bssid: bool,
    timeout_streak: u8,
) -> u32 {
    // Adaptive step is 8s to follow DHCP retry/backoff behavior without jumping
    // straight to max timeout on the first stall.
    // Source: RFC 2131 section 4.1 https://datatracker.ietf.org/doc/html/rfc2131#section-4.1
    const WIFI_DHCP_TIMEOUT_ADAPTIVE_STEP_MS: u32 = 8_000;
    // Cap adaptive extension to keep each round bounded and let the recovery
    // ladder (candidate/auth/driver) progress deterministically.
    const WIFI_DHCP_TIMEOUT_ADAPTIVE_MAX_STEPS: u8 = 4;

    let base_timeout_ms = runtime_policy.dhcp_timeout_ms;
    let adaptive_steps = u32::from(timeout_streak.min(WIFI_DHCP_TIMEOUT_ADAPTIVE_MAX_STEPS));
    let adaptive_extra_ms = adaptive_steps.saturating_mul(WIFI_DHCP_TIMEOUT_ADAPTIVE_STEP_MS);
    let adaptive_timeout_ms = base_timeout_ms.saturating_add(adaptive_extra_ms);
    let pinned_cap_ms = if pinned_bssid {
        runtime_policy.pinned_dhcp_timeout_ms
    } else {
        WIFI_DHCP_TIMEOUT_MAX_MS
    };
    adaptive_timeout_ms
        .min(pinned_cap_ms)
        .min(WIFI_DHCP_TIMEOUT_MAX_MS)
}
