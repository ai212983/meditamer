//! The Wi-Fi configuration domain: credentials, runtime policy and its
//! sanitization, and the control vocabulary shared by the serial command
//! surface and network owner.
//!
//! Moved wholesale from `products/meditamer/src/firmware/types/wifi.rs`
//! (product/target axis completion plan, Phase 4). Unconditional now: the
//! whole `netstack` crate is behind the product's `asset-upload-http`
//! optional dependency, so nothing here needs its own `cfg` gate any more.

pub const WIFI_SSID_MAX: usize = 32;
pub const WIFI_PASSWORD_MAX: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WifiCredentials {
    pub ssid: [u8; WIFI_SSID_MAX],
    pub ssid_len: u8,
    pub password: [u8; WIFI_PASSWORD_MAX],
    pub password_len: u8,
}

// Policy baselines and clamps for UART-provisioned NETCFG values.
//
// Why these defaults:
// - `esp_wifi_connect()` is one-shot; robust reconnect/recovery must be in app logic.
//   Source: https://docs.espressif.com/projects/esp-idf/en/v5.3.1/esp32/api-reference/network/esp_wifi.html#_CPPv416esp_wifi_connectv
// - Scan dwell values are per-channel; passive dwell >1500ms is explicitly discouraged.
//   Source (esp-radio, this stack): https://docs.espressif.com/projects/rust/esp-radio/0.16.0/esp32s3/src/esp_radio/wifi/mod.rs.html
// - DHCP retries are expected to use exponential backoff windows.
//   Source: RFC 2131 section 4.1 https://datatracker.ietf.org/doc/html/rfc2131#section-4.1
pub const WIFI_DHCP_TIMEOUT_MIN_MS: u32 = 5_000;
pub const WIFI_DHCP_TIMEOUT_MAX_MS: u32 = 180_000;
// 20s: enough for several DHCP request/retry rounds on healthy APs without masking hangs.
pub const WIFI_DHCP_TIMEOUT_DEFAULT_MS: u32 = 20_000;
// 45s: pinned-BSSID recovery is intentionally given longer lease convergence budget.
// This reduces false failovers during reassociation after targeted channel/BSSID retries.
pub const WIFI_DHCP_TIMEOUT_PINNED_DEFAULT_MS: u32 = 45_000;
// 30s: bounded connect phase budget before policy ladder escalates recovery.
pub const WIFI_CONNECT_TIMEOUT_DEFAULT_MS: u32 = 30_000;
// 25s: listener start should trail successful lease, but not block recovery indefinitely.
pub const WIFI_LISTENER_TIMEOUT_DEFAULT_MS: u32 = 25_000;
// Active scan dwell is per channel; defaults bias toward reliability over fastest join.
pub const WIFI_SCAN_ACTIVE_MIN_DEFAULT_MS: u32 = 600;
pub const WIFI_SCAN_ACTIVE_MAX_DEFAULT_MS: u32 = 1_500;
// Keep passive dwell at 1500ms ceiling, matching esp-radio warning threshold.
pub const WIFI_SCAN_PASSIVE_DEFAULT_MS: u32 = 1_500;
// 1.2s cooldown avoids hot-loop retries while keeping recovery responsive.
pub const WIFI_COOLDOWN_DEFAULT_MS: u32 = 1_200;
// 2.5s between hard driver restarts gives radio/stack time to settle before re-init.
pub const WIFI_DRIVER_RESTART_BACKOFF_DEFAULT_MS: u32 = 2_500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WifiRuntimePolicy {
    pub connect_timeout_ms: u32,
    pub dhcp_timeout_ms: u32,
    pub pinned_dhcp_timeout_ms: u32,
    pub listener_timeout_ms: u32,
    pub scan_active_min_ms: u32,
    pub scan_active_max_ms: u32,
    pub scan_passive_ms: u32,
    pub retry_same_max: u8,
    pub rotate_candidate_max: u8,
    pub rotate_auth_max: u8,
    pub full_scan_reset_max: u8,
    pub driver_restart_max: u8,
    pub cooldown_ms: u32,
    pub driver_restart_backoff_ms: u32,
}

impl WifiRuntimePolicy {
    pub const fn defaults() -> Self {
        Self {
            connect_timeout_ms: WIFI_CONNECT_TIMEOUT_DEFAULT_MS,
            dhcp_timeout_ms: WIFI_DHCP_TIMEOUT_DEFAULT_MS,
            pinned_dhcp_timeout_ms: WIFI_DHCP_TIMEOUT_PINNED_DEFAULT_MS,
            listener_timeout_ms: WIFI_LISTENER_TIMEOUT_DEFAULT_MS,
            scan_active_min_ms: WIFI_SCAN_ACTIVE_MIN_DEFAULT_MS,
            scan_active_max_ms: WIFI_SCAN_ACTIVE_MAX_DEFAULT_MS,
            scan_passive_ms: WIFI_SCAN_PASSIVE_DEFAULT_MS,
            retry_same_max: 2,
            rotate_candidate_max: 2,
            rotate_auth_max: 5,
            full_scan_reset_max: 1,
            driver_restart_max: 1,
            cooldown_ms: WIFI_COOLDOWN_DEFAULT_MS,
            driver_restart_backoff_ms: WIFI_DRIVER_RESTART_BACKOFF_DEFAULT_MS,
        }
    }

    pub const fn sanitized(self) -> Self {
        let connect_timeout_ms = clamp_u32(self.connect_timeout_ms, 2_000, 180_000);
        let dhcp_timeout_ms = clamp_u32(
            self.dhcp_timeout_ms,
            WIFI_DHCP_TIMEOUT_MIN_MS,
            WIFI_DHCP_TIMEOUT_MAX_MS,
        );
        let mut pinned_dhcp_timeout_ms = clamp_u32(
            self.pinned_dhcp_timeout_ms,
            WIFI_DHCP_TIMEOUT_MIN_MS,
            WIFI_DHCP_TIMEOUT_MAX_MS,
        );
        if pinned_dhcp_timeout_ms < dhcp_timeout_ms {
            pinned_dhcp_timeout_ms = dhcp_timeout_ms;
        }
        let listener_timeout_ms = clamp_u32(self.listener_timeout_ms, 2_000, 180_000);
        let scan_active_min_ms = clamp_u32(self.scan_active_min_ms, 50, 10_000);
        let mut scan_active_max_ms = clamp_u32(self.scan_active_max_ms, 50, 10_000);
        if scan_active_max_ms < scan_active_min_ms {
            scan_active_max_ms = scan_active_min_ms;
        }
        let scan_passive_ms = clamp_u32(self.scan_passive_ms, 50, 10_000);
        let retry_same_max = clamp_u8(self.retry_same_max, 1, 8);
        let rotate_candidate_max = clamp_u8(self.rotate_candidate_max, 1, 8);
        let rotate_auth_max = clamp_u8(self.rotate_auth_max, 1, 16);
        let full_scan_reset_max = clamp_u8(self.full_scan_reset_max, 1, 8);
        let driver_restart_max = clamp_u8(self.driver_restart_max, 1, 8);
        let cooldown_ms = clamp_u32(self.cooldown_ms, 0, 30_000);
        let driver_restart_backoff_ms = clamp_u32(self.driver_restart_backoff_ms, 0, 60_000);
        Self {
            connect_timeout_ms,
            dhcp_timeout_ms,
            pinned_dhcp_timeout_ms,
            listener_timeout_ms,
            scan_active_min_ms,
            scan_active_max_ms,
            scan_passive_ms,
            retry_same_max,
            rotate_candidate_max,
            rotate_auth_max,
            full_scan_reset_max,
            driver_restart_max,
            cooldown_ms,
            driver_restart_backoff_ms,
        }
    }
}

const fn clamp_u32(value: u32, min: u32, max: u32) -> u32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

const fn clamp_u8(value: u8, min: u8, max: u8) -> u8 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetControlCommand {
    Recover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetConfigSet {
    pub credentials: Option<WifiCredentials>,
    pub policy: WifiRuntimePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiConfigResultCode {
    Ok,
    InvalidData,
    OperationFailed,
}
