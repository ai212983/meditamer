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
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};
use embassy_net::Stack;
use embassy_time::{with_timeout, Duration, Instant, Timer};
use esp_println::println;
use esp_radio::wifi::{
    event::{self, EventExt},
    AccessPointInfo, AuthMethod, ClientConfig, Config as WifiDriverConfig, InternalWifiError,
    ModeConfig, PowerSaveMode, ScanMethod, WifiController, WifiError,
};

// Cap scan result set to keep telemetry and candidate rotation bounded.
const WIFI_SCAN_DIAG_MAX_APS: usize = 64;
// Keep top-N BSSID candidates by RSSI for deterministic rotate-candidate recovery.
const WIFI_AP_CANDIDATE_MAX: usize = 8;
// Probe channel 8 first (lab/default AP channel), then sweep full 2.4GHz set.
// Channel universe rationale: IEEE 802.11 country-plan channels are represented
// by `wifi_country_t.schann/nchan` in Espressif Wi-Fi API.
// Source: https://docs.espressif.com/projects/esp-idf/en/v5.3.1/esp32/api-reference/network/esp_wifi.html
const WIFI_CHANNEL_PROBE_SEQUENCE: [u8; 13] = [8, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13];
// Bounded fallback for repeated all-channel zero-result scans.
const WIFI_ZERO_DISCOVERY_SCAN_PROBE_CHANNELS: [u8; 4] = [8, 1, 6, 11];
// Hard guard against zero-discovery loops: after two consecutive full-sweep
// zero-result cycles, force hard restart and full-channel probe escalation.
// Keep this separate from policy knobs so refactors/tuning cannot accidentally
// disable the safety path.
const WIFI_ZERO_DISCOVERY_HARD_GUARD_STREAK: u8 = 2;
// Bound hard-restart escalations before declaring terminal discovery failure.
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
const WIFI_REASON_DHCP_NO_IPV4_STALL: u8 = 250;
const WIFI_REASON_POST_HARD_RECOVER_CONNECT_STALL: u8 = 251;
const WIFI_REASON_CONNECT_ATTEMPT_TIMEOUT: u8 = 252;
const WIFI_REASON_START_NOMEM: u8 = 253;
const WIFI_REASON_SCAN_NOMEM: u8 = 254;
// Upper bound for driver control calls in recovery paths; prevents indefinite
// task stalls if the radio stack stops responding.
// Chosen so stop/disconnect can complete under transient RF contention while
// still bounding host-observed NET_STATUS staleness.
const WIFI_DRIVER_CONTROL_TIMEOUT_MS: u64 = 5_000;
// Stop can transiently report timeout while the driver is unwinding
// internal work; retry with short backoff before declaring failure.
const WIFI_DRIVER_STOP_RETRIES: u8 = 2;
const WIFI_DRIVER_STOP_RETRY_BACKOFF_MS: u64 = 300;
// Poll cadence while connected to detect disconnect/lease/listener transitions
// without creating hot-loop UART noise.
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
const WIFI_POST_START_SETTLE_MS: u64 = 800;
// Settle after disconnect event to let stop/disconnect complete before re-entering connect path.
const WIFI_POST_DISCONNECT_SETTLE_MS: u64 = 1_200;

static WIFI_EVENT_LOGGER_INSTALLED: AtomicBool = AtomicBool::new(false);
static WIFI_LAST_DISCONNECT_REASON: AtomicU8 = AtomicU8::new(0);
static WIFI_DISCONNECTED_EVENT: AtomicBool = AtomicBool::new(false);
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
}

mod connect;
mod diag;
mod driver;
mod helpers;
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

pub(super) fn wifi_runtime_config() -> WifiDriverConfig {
    WifiDriverConfig::default()
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
