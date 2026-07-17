use super::super::super::{
    config::{
        NET_CONFIG_SET_UPDATES, NET_CONTROL_COMMANDS, WIFI_CREDENTIALS_UPDATES,
        WIFI_RUNTIME_POLICY_UPDATES,
    },
    psram,
    runtime::service_mode,
    telemetry,
    types::{
        NetControlCommand, WifiCredentials, WifiRuntimePolicy, WIFI_PASSWORD_MAX, WIFI_SSID_MAX,
    },
};
use core::{
    fmt::Write as _,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering},
};
use embassy_net::Stack;
use embassy_time::{with_timeout, Duration, Instant, Timer};
use esp_println::println;
mod backend;
mod runtime_init;
pub(crate) use backend::{
    backend_name, wifi_active_scan_config, wifi_channel_active_scan_config,
    wifi_client_mode_config, wifi_connect_async, wifi_directed_active_scan_config,
    wifi_disconnect_async, wifi_error_is_no_mem, wifi_is_connected, wifi_is_started,
    wifi_passive_scan_config, wifi_power_save_none, wifi_raw_broad_scan_config, wifi_rssi,
    wifi_scan_with_config_async, wifi_set_config, wifi_set_mode, wifi_set_power_saving,
    wifi_set_protocol, wifi_sta_mode, wifi_standard_bgn_protocols, wifi_start_async,
    wifi_stop_async, ScanConfig, WifiController, WifiDevice,
};
use backend::{AccessPointInfo, AuthMethod, ModeConfig};
pub(crate) use runtime_init::{apply_runtime_setup_overrides_and_log, initialize_runtime_sta};
type WifiError = backend::WifiError;
const WIFI_SCAN_DIAG_MAX_APS: usize = 64;
const WIFI_AP_CANDIDATE_MAX: usize = 8;
const WIFI_CHANNEL_PROBE_SEQUENCE: [u8; 13] = [8, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13];
const WIFI_ZERO_DISCOVERY_SCAN_PROBE_CHANNELS: [u8; 4] = [8, 1, 6, 11];
const WIFI_ZERO_DISCOVERY_HARD_GUARD_STREAK: u8 = 2;
const WIFI_ZERO_DISCOVERY_HARD_GUARD_MAX_RESTARTS: u8 = 2;
const WIFI_AUTH_METHODS: [AuthMethod; 5] = [
    AuthMethod::WpaWpa2Personal,
    AuthMethod::Wpa2Personal,
    AuthMethod::Wpa2Wpa3Personal,
    AuthMethod::Wpa3Personal,
    AuthMethod::Wpa,
];
// Values below mirror Espressif `wifi_err_reason_t` codes.
// Source: https://github.com/espressif/esp-idf/blob/v5.3/components/esp_wifi/include/esp_wifi_types_generic.h
const WIFI_REASON_BEACON_TIMEOUT: u8 = 200;
// Espressif reason 2 == auth expire; we preserve legacy "other" handling path.
const WIFI_REASON_OTHER: u8 = 2;
const WIFI_REASON_NO_AP_FOUND: u8 = 201;
const WIFI_REASON_AUTH_FAIL: u8 = 202;
const WIFI_REASON_ASSOC_FAIL: u8 = 203;
const WIFI_REASON_HANDSHAKE_TIMEOUT: u8 = 204;
const WIFI_REASON_CONNECTION_FAIL: u8 = 205;
const WIFI_REASON_NO_AP_FOUND_COMPAT_SECURITY: u8 = 210;
const WIFI_REASON_NO_AP_FOUND_AUTHMODE_THRESHOLD: u8 = 211;
const WIFI_REASON_NO_AP_FOUND_RSSI_THRESHOLD: u8 = 212;
const WIFI_REASON_CONNECT_LOW_INTERNAL_MEM: u8 = 249;
const WIFI_REASON_DHCP_NO_IPV4_STALL: u8 = 250;
const WIFI_REASON_POST_HARD_RECOVER_CONNECT_STALL: u8 = 251;
const WIFI_REASON_CONNECT_ATTEMPT_TIMEOUT: u8 = 252;
const WIFI_REASON_START_NOMEM: u8 = 253;
const WIFI_REASON_SCAN_NOMEM: u8 = 254;
const WIFI_DRIVER_CONTROL_TIMEOUT_MS: u64 = 5_000;
const WIFI_CONNECTED_WATCHDOG_MS: u64 = 2_000;
// Two bounded same-link reacquire attempts before escalating to candidate/auth rotation.
const WIFI_DHCP_LEASE_REACQUIRE_MAX_ATTEMPTS: u8 = 2;
// Backoff between lease-reacquire attempts; short by design to keep retries
// responsive without hot-looping the driver.
const WIFI_DHCP_LEASE_REACQUIRE_BACKOFF_MS: u64 = 800;
// If lease drops while already in ListenerWait, escalate quickly through the
// DHCP/no-IPv4 recovery path instead of waiting the full listener timeout.
const WIFI_LISTENER_LEASE_LOSS_GRACE_MS: u64 = 2_500;
// If we keep stalling on the same candidate twice, force hard restart/rescan.
const WIFI_DHCP_SAME_CANDIDATE_RESTART_STREAK: u8 = 2;
// Escalate recurring reason=2/auth-expire disconnects into hard recover after 3 hits.
const WIFI_REASON_OTHER_HARD_RECOVER_STREAK: u8 = 3;
// Post-hard-recover escalated sweep budget across auth/scan variants.
const WIFI_ESCALATED_AUTH_SWEEP_ATTEMPTS: u8 = 5;
// Short settle delay used between fast state transitions; prevents tight
// command/reconfigure loops from outracing radio task/event processing.
const WIFI_SHORT_SETTLE_MS: u64 = 500;
// Wait window for initial credentials provisioning over UART before retrying.
const WIFI_WAIT_CREDENTIALS_TIMEOUT_S: u64 = 3;
// Generic bounded retry backoff for recovery ladder transitions.
// Keeps reconnect strategy responsive while avoiding rapid retry oscillation.
const WIFI_RECOVERY_RETRY_BACKOFF_MS: u64 = 2_000;
// Extra backoff after driver start NoMem to give allocator/radio state time to recover.
const WIFI_NOMEM_RECOVERY_BACKOFF_MS: u64 = 5_000;
// Settle delay after successful start before connect/scan to avoid immediate
// post-start flakiness in early driver transition window.
const WIFI_POST_START_SETTLE_MS_DEFAULT: u64 = 800;
const WIFI_POST_START_SETTLE_MS_MIN: u64 = 100;
const WIFI_POST_START_SETTLE_MS_MAX: u64 = 10_000;
const WIFI_POST_START_SETTLE_MS: u64 = {
    let configured = match option_env!("MEDITAMER_WIFI_POST_START_SETTLE_MS") {
        Some(value) => Some(value),
        None => option_env!("WIFI_POST_START_SETTLE_MS"),
    };
    match configured {
        Some(raw) => match parse_ascii_u64(raw) {
            Some(value)
                if value >= WIFI_POST_START_SETTLE_MS_MIN
                    && value <= WIFI_POST_START_SETTLE_MS_MAX =>
            {
                value
            }
            _ => WIFI_POST_START_SETTLE_MS_DEFAULT,
        },
        None => WIFI_POST_START_SETTLE_MS_DEFAULT,
    }
};
const WIFI_START_RAW_SCAN_DIAG_TIMEOUT_MS: u64 = 12_000;
const WIFI_START_RAW_SCAN_DIAG: bool =
    parse_nonzero_flag(match option_env!("MEDITAMER_WIFI_START_RAW_SCAN_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_START_RAW_SCAN_DIAG"),
    });

pub(crate) fn boot_scan_only_diag_enabled() -> bool {
    connect::boot_scan_only_diag_enabled()
}
const WIFI_START_READINESS_PROBE: bool =
    parse_nonzero_flag(match option_env!("MEDITAMER_WIFI_START_READINESS_PROBE") {
        Some(value) => Some(value),
        None => option_env!("WIFI_START_READINESS_PROBE"),
    });
const WIFI_FORCE_STOP_BEFORE_START: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_FORCE_STOP_BEFORE_START") {
        Some(value) => Some(value),
        None => option_env!("WIFI_FORCE_STOP_BEFORE_START"),
    },
);
const WIFI_COUNTRY_US_OVERRIDE: bool =
    parse_nonzero_flag(match option_env!("MEDITAMER_WIFI_COUNTRY_US_OVERRIDE") {
        Some(value) => Some(value),
        None => option_env!("WIFI_COUNTRY_US_OVERRIDE"),
    });
const WIFI_START_READINESS_PROBE_STEPS_MS: [u64; 3] = [0, 200, 800];
const WIFI_POST_DISCONNECT_SETTLE_MS: u64 = 1_200;

static WIFI_EVENT_LOGGER_INSTALLED: AtomicBool = AtomicBool::new(false);
static WIFI_LAST_DISCONNECT_REASON: AtomicU8 = AtomicU8::new(0);
static WIFI_DISCONNECTED_EVENT: AtomicBool = AtomicBool::new(false);
static WIFI_LAST_STA_START_AT_MS: AtomicU32 = AtomicU32::new(0);
static WIFI_LAST_STA_STOP_AT_MS: AtomicU32 = AtomicU32::new(0);
static WIFI_LAST_SCAN_DONE_AT_MS: AtomicU32 = AtomicU32::new(0);
static WIFI_LAST_SCAN_DONE_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_LAST_SCAN_DONE_ID: AtomicU32 = AtomicU32::new(0);
static WIFI_LAST_SCAN_DONE_STATUS: AtomicU32 = AtomicU32::new(0);
const DIAG_WIFI: u32 = telemetry::DIAG_DOMAIN_WIFI;
const DIAG_REASSOC: u32 = telemetry::DIAG_DOMAIN_REASSOC;

macro_rules! diag_wifi {
    ($($arg:tt)*) => {
        if telemetry::diag_enabled(DIAG_WIFI) {
            println!($($arg)*);
        }
    };
}

macro_rules! diag_reassoc {
    ($($arg:tt)*) => {
        if telemetry::diag_enabled(DIAG_REASSOC) {
            println!($($arg)*);
        }
    };
}

const fn parse_ascii_u64(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut idx = 0;
    let mut parsed = 0u64;
    while idx < bytes.len() {
        let ch = bytes[idx];
        if ch < b'0' || ch > b'9' {
            return None;
        }
        parsed = parsed.saturating_mul(10).saturating_add((ch - b'0') as u64);
        idx += 1;
    }
    Some(parsed)
}

const fn parse_nonzero_flag(value: Option<&str>) -> bool {
    matches!(value, Some(raw) if matches!(parse_ascii_u64(raw), Some(parsed) if parsed > 0))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TargetApHint {
    channel: u8,
    bssid: [u8; 6],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TargetApCandidate {
    hint: TargetApHint,
    rssi: i8,
}

struct ScanOutcome {
    candidates: heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    hit_nomem: bool,
    saw_nonzero_results: bool,
    saw_target_candidate: bool,
}

mod connect;
mod diag;
mod driver;
mod helpers;
mod legacy_discovery;
mod policy;
mod scan;
mod scan_candidates;
mod state;
mod task;

use connect::{
    active_scan_timeout_ms, directed_scan_timeout_ms, passive_scan_timeout_ms,
    run_wifi_connection_task as run_wifi_connection_task_inner, wifi_credentials,
    wifi_credentials_from_parts, zero_discovery_probe_timeout_ms,
};
use diag::{
    net_config_snapshot as diag_net_config_snapshot, publish_config, publish_state,
    read_status_fields,
};
use helpers::{
    elapsed_ms_u32, format_bssid, format_bssid_opt, has_ipv4_lease, is_no_mem_wifi_error,
    log_radio_mem_diag, log_radio_mem_diag_with_trigger, policy_total_attempt_budget,
    stack_ipv4_lease,
};
use policy::effective_dhcp_timeout_ms;
use scan::scan_target_candidates;
use scan_candidates::rotate_to_next_candidate;
use state::{NetFailureClass, NetState, NetStatusSnapshot, RecoveryLadderStep};

pub(super) async fn run_wifi_connection_task(
    controller: WifiController<'static>,
    credentials: Option<WifiCredentials>,
    stack: Stack<'static>,
) {
    run_wifi_connection_task_inner(controller, credentials, stack).await;
}

pub(super) fn compiled_wifi_credentials() -> Option<WifiCredentials> {
    wifi_credentials().and_then(|(ssid, password)| {
        wifi_credentials_from_parts(ssid.as_bytes(), password.as_bytes()).ok()
    })
}

pub(crate) struct NetConfigSnapshotView {
    pub(crate) credentials_set: bool,
    pub(crate) ssid: heapless::String<WIFI_SSID_MAX>,
    pub(crate) policy: WifiRuntimePolicy,
}

pub(crate) fn net_config_snapshot() -> NetConfigSnapshotView {
    let snapshot = diag_net_config_snapshot();
    let mut ssid = heapless::String::<WIFI_SSID_MAX>::new();
    let ssid_len = snapshot.ssid_len.min(WIFI_SSID_MAX as u8) as usize;
    for byte in snapshot.ssid[..ssid_len].iter().copied() {
        let _ = ssid.push(byte as char);
    }
    NetConfigSnapshotView {
        credentials_set: snapshot.credentials_set,
        ssid,
        policy: snapshot.policy,
    }
}

pub(crate) fn net_status_snapshot() -> NetStatusSnapshot {
    let (state, ladder_step, attempt, failure_class, failure_code, uptime_ms) =
        read_status_fields();
    let telemetry = telemetry::snapshot();
    NetStatusSnapshot {
        state: state.as_str(),
        link: telemetry.wifi_link_connected,
        radio_quiesced: diag::radio_quiesced(),
        ipv4: telemetry.upload_http_ipv4.unwrap_or([0, 0, 0, 0]),
        listener: telemetry.upload_http_listening,
        listener_enabled: service_mode::upload_http_listener_enabled(),
        failure_class: failure_class.as_str(),
        failure_code,
        ladder_step: ladder_step.as_str(),
        attempt,
        uptime_ms,
    }
}
