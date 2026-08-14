use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetPolicy {
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

impl Default for NetPolicy {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 30_000,
            dhcp_timeout_ms: 20_000,
            pinned_dhcp_timeout_ms: 45_000,
            listener_timeout_ms: 25_000,
            scan_active_min_ms: 600,
            scan_active_max_ms: 1_500,
            scan_passive_ms: 1_500,
            retry_same_max: 2,
            rotate_candidate_max: 2,
            rotate_auth_max: 5,
            full_scan_reset_max: 1,
            driver_restart_max: 1,
            cooldown_ms: 1_200,
            driver_restart_backoff_ms: 2_500,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct NetStatus {
    pub state: Option<String>,
    pub link: Option<bool>,
    pub ipv4: Option<String>,
    pub listener: Option<bool>,
    pub listener_enabled: Option<bool>,
    pub failure_class: Option<String>,
    pub failure_code: Option<u64>,
    pub ladder_step: Option<String>,
    pub attempt: Option<u64>,
    pub uptime_ms: Option<u64>,
}
