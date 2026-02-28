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
mod diag;
mod driver;
mod policy;
mod state;
mod task;

use diag::{
    net_config_snapshot as diag_net_config_snapshot, publish_config, publish_state,
    read_status_fields,
};
use policy::effective_dhcp_timeout_ms;
use state::{NetFailureClass, NetState, NetStatusSnapshot, RecoveryLadderStep};
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
pub(super) async fn run_wifi_connection_task(
    mut controller: WifiController<'static>,
    mut credentials: Option<WifiCredentials>,
    stack: Stack<'static>,
) {
    install_wifi_event_logger();
    telemetry::set_wifi_link_connected(false);
    let started_at = Instant::now();
    let mut net_state = NetState::Idle;
    let mut ladder_step = RecoveryLadderStep::RetrySame;
    let mut net_attempt = 0u32;
    let mut failure_class = NetFailureClass::None;
    let mut failure_code = 0u8;
    let mut config_applied = false;
    let mut auth_method_idx = 0usize;
    let mut paused = false;
    let mut channel_hint = None;
    let mut bssid_hint = None;
    let mut ap_candidates = heapless::Vec::<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>::new();
    let mut ap_candidate_idx = 0usize;
    let mut dhcp_same_candidate_timeout_streak = 0u8;
    let mut dhcp_lease_reacquire_attempts = 0u8;
    let mut other_disconnect_streak = 0u8;
    let mut channel_probe_idx = 0usize;
    let mut hard_recover_watchdog_started_at: Option<Instant> = None;
    let mut escalated_auth_sweep_attempts_left = 0u8;
    let mut runtime_policy = WifiRuntimePolicy::defaults().sanitized();
    let mut terminal_fail_latched = false;
    publish_config(credentials, runtime_policy);
    publish_state(
        net_state,
        ladder_step,
        net_attempt,
        failure_class,
        failure_code,
        started_at.elapsed().as_millis() as u32,
    );
    if credentials.is_none() {
        diag_wifi!("upload_http: waiting for NETCFG credentials over UART");
    }
    loop {
        apply_pending_runtime_policy_updates(&mut runtime_policy);
        while let Ok(config) = NET_CONFIG_SET_UPDATES.try_receive() {
            runtime_policy = config.policy.sanitized();
            if let Some(updated) = config.credentials {
                if credentials != Some(updated) {
                    credentials = Some(updated);
                    telemetry::record_wifi_reassoc_credentials_changed();
                }
            }
            net_attempt = 0;
            terminal_fail_latched = false;
            publish_config(credentials, runtime_policy);
        }
        while let Ok(control) = NET_CONTROL_COMMANDS.try_receive() {
            if matches!(control, NetControlCommand::Recover) {
                // Do not call stop/disconnect directly from host-triggered recover.
                // These driver calls can wedge in pathological radio states, leaving
                // the task stuck and host seeing a stale NET_STATUS forever.
                // We perform a soft recover here and let the normal event loop
                // drive reconnect/restart policy steps.
                // Invariant: keep this path idempotent and non-blocking; host may
                // issue repeated NET RECOVER while telemetry is being sampled.
                config_applied = false;
                auth_method_idx = 0;
                channel_hint = None;
                bssid_hint = None;
                ap_candidates.clear();
                ap_candidate_idx = 0;
                channel_probe_idx = 0;
                dhcp_same_candidate_timeout_streak = 0;
                dhcp_lease_reacquire_attempts = 0;
                other_disconnect_streak = 0;
                hard_recover_watchdog_started_at = Some(Instant::now());
                escalated_auth_sweep_attempts_left = 0;
                terminal_fail_latched = false;
                net_attempt = 0;
                // Reset failure envelope so the next loop iteration is evaluated as
                // a fresh recovery attempt, not as continuation of a stale failure.
                ladder_step = RecoveryLadderStep::DriverRestart;
                failure_class = NetFailureClass::None;
                failure_code = 0;
                transition_state(
                    &mut net_state,
                    NetState::Recovering,
                    "host_recover",
                    started_at,
                    ladder_step,
                    net_attempt,
                    (failure_class, failure_code),
                );
                publish_state(
                    net_state,
                    ladder_step,
                    net_attempt,
                    failure_class,
                    failure_code,
                    started_at.elapsed().as_millis() as u32,
                );
            }
        }
        publish_config(credentials, runtime_policy);

        if !service_mode::upload_enabled() {
            if !paused {
                disconnect_and_stop_with_timeout(&mut controller, "upload_off_pause").await;
                telemetry::set_wifi_link_connected(false);
                telemetry::set_upload_http_listener(false, None);
                telemetry::record_wifi_reassoc_mode_pause();
                paused = true;
                config_applied = false;
                auth_method_idx = 0;
                channel_hint = None;
                bssid_hint = None;
                ap_candidates.clear();
                ap_candidate_idx = 0;
                channel_probe_idx = 0;
                dhcp_lease_reacquire_attempts = 0;
                other_disconnect_streak = 0;
                hard_recover_watchdog_started_at = None;
                escalated_auth_sweep_attempts_left = 0;
                terminal_fail_latched = false;
                net_attempt = 0;
                diag_wifi!("upload_http: upload mode off; wifi paused");
                transition_state(
                    &mut net_state,
                    NetState::Idle,
                    "upload_off",
                    started_at,
                    ladder_step,
                    net_attempt,
                    (failure_class, failure_code),
                );
                publish_state(
                    net_state,
                    ladder_step,
                    net_attempt,
                    failure_class,
                    failure_code,
                    started_at.elapsed().as_millis() as u32,
                );
            }
            Timer::after(Duration::from_millis(WIFI_SHORT_SETTLE_MS)).await;
            continue;
        }

        if paused {
            paused = false;
            net_attempt = 0;
            terminal_fail_latched = false;
            telemetry::record_wifi_reassoc_mode_resume();
            diag_wifi!("upload_http: upload mode on; wifi resuming");
            transition_state(
                &mut net_state,
                NetState::Starting,
                "upload_on",
                started_at,
                ladder_step,
                net_attempt,
                (failure_class, failure_code),
            );
        }
        if terminal_fail_latched {
            Timer::after(Duration::from_millis(
                runtime_policy.cooldown_ms.max(250) as u64
            ))
            .await;
            continue;
        }
        if let Some(watchdog_started_at) = hard_recover_watchdog_started_at {
            let elapsed_ms = watchdog_started_at.elapsed().as_millis();
            // Guardrail: do not key this watchdog to connect_timeout alone.
            // Discovery includes an explicit scan phase that can legitimately
            // outlive connect_timeout on dense 2.4 GHz environments.
            let watchdog_timeout_ms = post_recover_watchdog_timeout_ms(runtime_policy);
            if elapsed_ms >= watchdog_timeout_ms {
                telemetry::record_wifi_reassoc_disconnect_event(
                    WIFI_REASON_POST_HARD_RECOVER_CONNECT_STALL,
                );
                println!(
                    "upload_http: post-hard-recover-connect-stall elapsed_ms={} watchdog_timeout_ms={} connect_timeout_ms={} forcing full restart",
                    elapsed_ms,
                    watchdog_timeout_ms,
                    runtime_policy.connect_timeout_ms
                );
                disconnect_and_stop_with_timeout(&mut controller, "post_recover_watchdog").await;
                telemetry::set_wifi_link_connected(false);
                telemetry::set_upload_http_listener(false, None);
                config_applied = false;
                auth_method_idx = 0;
                channel_hint = None;
                bssid_hint = None;
                ap_candidates.clear();
                ap_candidate_idx = 0;
                channel_probe_idx = 0;
                dhcp_same_candidate_timeout_streak = 0;
                dhcp_lease_reacquire_attempts = 0;
                other_disconnect_streak = 0;
                hard_recover_watchdog_started_at = Some(Instant::now());
                escalated_auth_sweep_attempts_left = WIFI_ESCALATED_AUTH_SWEEP_ATTEMPTS;
                ladder_step = RecoveryLadderStep::DriverRestart;
                failure_class = NetFailureClass::PostRecoverStall;
                failure_code = WIFI_REASON_POST_HARD_RECOVER_CONNECT_STALL;
                transition_state(
                    &mut net_state,
                    NetState::Recovering,
                    "post_recover_watchdog",
                    started_at,
                    ladder_step,
                    net_attempt,
                    (failure_class, failure_code),
                );
                publish_state(
                    net_state,
                    ladder_step,
                    net_attempt,
                    failure_class,
                    failure_code,
                    started_at.elapsed().as_millis() as u32,
                );
                println!(
                    "upload_http: post-hard-recover-escalated-scan begin attempts={} watchdog_timeout_ms={} connect_timeout_ms={}",
                    escalated_auth_sweep_attempts_left,
                    watchdog_timeout_ms,
                    runtime_policy.connect_timeout_ms
                );
                Timer::after(Duration::from_millis(
                    runtime_policy.driver_restart_backoff_ms as u64,
                ))
                .await;
                continue;
            }
        }

        while let Ok(updated) = WIFI_CREDENTIALS_UPDATES.try_receive() {
            if credentials == Some(updated) {
                diag_wifi!("upload_http: wifi credentials unchanged; skipping reconfigure");
                continue;
            }
            credentials = Some(updated);
            config_applied = false;
            auth_method_idx = 0;
            channel_hint = None;
            bssid_hint = None;
            ap_candidates.clear();
            ap_candidate_idx = 0;
            channel_probe_idx = 0;
            dhcp_lease_reacquire_attempts = 0;
            other_disconnect_streak = 0;
            hard_recover_watchdog_started_at = None;
            escalated_auth_sweep_attempts_left = 0;
            net_attempt = 0;
            terminal_fail_latched = false;
            telemetry::record_wifi_reassoc_credentials_changed();
            diag_wifi!("upload_http: wifi credentials updated");
        }

        let active = match credentials {
            Some(value) => value,
            None => {
                if let Ok(first) = with_timeout(
                    Duration::from_secs(WIFI_WAIT_CREDENTIALS_TIMEOUT_S),
                    WIFI_CREDENTIALS_UPDATES.receive(),
                )
                .await
                {
                    credentials = Some(first);
                    config_applied = false;
                    auth_method_idx = 0;
                    channel_hint = None;
                    bssid_hint = None;
                    ap_candidates.clear();
                    ap_candidate_idx = 0;
                    channel_probe_idx = 0;
                    dhcp_lease_reacquire_attempts = 0;
                    other_disconnect_streak = 0;
                    hard_recover_watchdog_started_at = None;
                    escalated_auth_sweep_attempts_left = 0;
                    net_attempt = 0;
                    terminal_fail_latched = false;
                    telemetry::record_wifi_reassoc_credentials_received();
                    publish_config(credentials, runtime_policy);
                    diag_wifi!("upload_http: wifi credentials received");
                }
                continue;
            }
        };

        if net_attempt >= policy_total_attempt_budget(runtime_policy) {
            terminal_fail_latched = true;
            ladder_step = RecoveryLadderStep::TerminalFail;
            if matches!(failure_class, NetFailureClass::None) {
                failure_class = NetFailureClass::Unknown;
            }
            if failure_code == 0 {
                failure_code = WIFI_REASON_OTHER;
            }
            transition_state(
                &mut net_state,
                NetState::Failed,
                "attempt_budget_exhausted",
                started_at,
                ladder_step,
                net_attempt,
                (failure_class, failure_code),
            );
            publish_state(
                net_state,
                ladder_step,
                net_attempt,
                failure_class,
                failure_code,
                started_at.elapsed().as_millis() as u32,
            );
            Timer::after(Duration::from_millis(
                runtime_policy.cooldown_ms.max(250) as u64
            ))
            .await;
            continue;
        }

        if !config_applied {
            let auth_method = WIFI_AUTH_METHODS[auth_method_idx];
            let mode =
                match mode_config_from_credentials(active, auth_method, channel_hint, bssid_hint) {
                    Some(mode) => mode,
                    None => {
                        diag_wifi!("upload_http: wifi credentials invalid utf8 or length");
                        credentials = None;
                        continue;
                    }
                };

            if let Err(err) = controller.set_config(&mode) {
                diag_wifi!("upload_http: wifi station config err={:?}", err);
                if matches!(controller.is_started(), Ok(true)) {
                    let _ = controller.stop_async().await;
                }
                config_applied = false;
                Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                continue;
            }
            diag_reassoc!(
                "upload_http: applying station config auth={:?} channel_hint={:?} bssid_hint={}",
                auth_method,
                channel_hint,
                format_bssid_opt(bssid_hint),
            );
            telemetry::record_wifi_reassoc_config_applied(
                auth_method_idx,
                channel_hint,
                channel_probe_idx,
            );
            config_applied = true;
        }

        match controller.is_started() {
            Ok(true) => {}
            Ok(false) => {
                transition_state(
                    &mut net_state,
                    NetState::Starting,
                    "start_driver",
                    started_at,
                    ladder_step,
                    net_attempt,
                    (failure_class, failure_code),
                );
                log_radio_mem_diag("start_before");
                if let Err(err) = controller.start_async().await {
                    diag_wifi!("upload_http: wifi start err={:?}", err);
                    log_radio_mem_diag("start_err");
                    telemetry::record_wifi_reassoc_start_err();
                    config_applied = false;
                    failure_class = NetFailureClass::Transport;
                    failure_code = WIFI_REASON_OTHER;
                    ladder_step = RecoveryLadderStep::DriverRestart;
                    transition_state(
                        &mut net_state,
                        NetState::Recovering,
                        "start_err",
                        started_at,
                        ladder_step,
                        net_attempt,
                        (failure_class, failure_code),
                    );
                    publish_state(
                        net_state,
                        ladder_step,
                        net_attempt,
                        failure_class,
                        failure_code,
                        started_at.elapsed().as_millis() as u32,
                    );
                    if hard_recover_watchdog_started_at.is_none() {
                        hard_recover_watchdog_started_at = Some(Instant::now());
                    }
                    if is_no_mem_wifi_error(&err) {
                        disconnect_and_stop_with_timeout(&mut controller, "start_nomem").await;
                        channel_hint = None;
                        bssid_hint = None;
                        ap_candidates.clear();
                        ap_candidate_idx = 0;
                        auth_method_idx = 0;
                        channel_probe_idx = 0;
                        dhcp_same_candidate_timeout_streak = 0;
                        dhcp_lease_reacquire_attempts = 0;
                        other_disconnect_streak = 0;
                        diag_reassoc!(
                            "upload_http: wifi start NoMem; forcing full wifi reset and hint clear"
                        );
                        log_radio_mem_diag("start_nomem");
                        ladder_step = RecoveryLadderStep::DriverRestart;
                        failure_class = NetFailureClass::Transport;
                        failure_code = WIFI_REASON_START_NOMEM;
                        transition_state(
                            &mut net_state,
                            NetState::Recovering,
                            "start_nomem",
                            started_at,
                            ladder_step,
                            net_attempt,
                            (failure_class, failure_code),
                        );
                        publish_state(
                            net_state,
                            ladder_step,
                            net_attempt,
                            failure_class,
                            failure_code,
                            started_at.elapsed().as_millis() as u32,
                        );
                        Timer::after(Duration::from_millis(WIFI_NOMEM_RECOVERY_BACKOFF_MS)).await;
                        continue;
                    }
                    disconnect_and_stop_with_timeout(&mut controller, "start_err").await;
                    Timer::after(Duration::from_millis(
                        runtime_policy.driver_restart_backoff_ms as u64,
                    ))
                    .await;
                    continue;
                }
                log_radio_mem_diag("start_ok");
                if let Err(err) = controller.set_power_saving(PowerSaveMode::None) {
                    diag_wifi!("upload_http: wifi set power save none err={:?}", err);
                }
                telemetry::record_wifi_reassoc_start_ok();
                Timer::after(Duration::from_millis(WIFI_POST_START_SETTLE_MS)).await;
            }
            Err(err) => {
                diag_wifi!("upload_http: wifi status err={:?}", err);
                failure_class = NetFailureClass::Transport;
                failure_code = WIFI_REASON_OTHER;
                ladder_step = RecoveryLadderStep::DriverRestart;
                transition_state(
                    &mut net_state,
                    NetState::Recovering,
                    "status_err",
                    started_at,
                    ladder_step,
                    net_attempt,
                    (failure_class, failure_code),
                );
                publish_state(
                    net_state,
                    ladder_step,
                    net_attempt,
                    failure_class,
                    failure_code,
                    started_at.elapsed().as_millis() as u32,
                );
                if hard_recover_watchdog_started_at.is_none() {
                    hard_recover_watchdog_started_at = Some(Instant::now());
                }
                let _ = controller.disconnect_async().await;
                let _ = controller.stop_async().await;
                config_applied = false;
                Timer::after(Duration::from_millis(
                    runtime_policy.driver_restart_backoff_ms as u64,
                ))
                .await;
                continue;
            }
        }
        if escalated_auth_sweep_attempts_left > 0 {
            channel_hint = None;
            bssid_hint = None;
            ap_candidates.clear();
            ap_candidate_idx = 0;
            channel_probe_idx = 0;
        }
        if channel_hint.is_none() && escalated_auth_sweep_attempts_left == 0 {
            transition_state(
                &mut net_state,
                NetState::Scanning,
                "scan_candidates",
                started_at,
                ladder_step,
                net_attempt,
                (failure_class, failure_code),
            );
            if let Ok(ssid) = core::str::from_utf8(&active.ssid[..active.ssid_len as usize]) {
                let scan_outcome =
                    scan_target_candidates(&mut controller, ssid, runtime_policy).await;
                if scan_outcome.hit_nomem {
                    failure_class = NetFailureClass::Transport;
                    failure_code = WIFI_REASON_SCAN_NOMEM;
                    ladder_step = RecoveryLadderStep::DriverRestart;
                    transition_state(
                        &mut net_state,
                        NetState::Recovering,
                        "scan_nomem",
                        started_at,
                        ladder_step,
                        net_attempt,
                        (failure_class, failure_code),
                    );
                    publish_state(
                        net_state,
                        ladder_step,
                        net_attempt,
                        failure_class,
                        failure_code,
                        started_at.elapsed().as_millis() as u32,
                    );
                    disconnect_and_stop_with_timeout(&mut controller, "scan_nomem").await;
                    telemetry::set_wifi_link_connected(false);
                    telemetry::set_upload_http_listener(false, None);
                    config_applied = false;
                    channel_hint = None;
                    bssid_hint = None;
                    ap_candidates.clear();
                    ap_candidate_idx = 0;
                    channel_probe_idx = 0;
                    auth_method_idx = 0;
                    dhcp_same_candidate_timeout_streak = 0;
                    dhcp_lease_reacquire_attempts = 0;
                    other_disconnect_streak = 0;
                    if hard_recover_watchdog_started_at.is_none() {
                        hard_recover_watchdog_started_at = Some(Instant::now());
                    }
                    Timer::after(Duration::from_millis(WIFI_NOMEM_RECOVERY_BACKOFF_MS)).await;
                    continue;
                }
                let scanned_candidates = scan_outcome.candidates;
                if let Some(candidate) = scanned_candidates.first().copied() {
                    ap_candidates = scanned_candidates;
                    ap_candidate_idx = 0;
                    channel_hint = Some(candidate.hint.channel);
                    bssid_hint = Some(candidate.hint.bssid);
                    auth_method_idx = 0;
                    config_applied = false;
                    channel_probe_idx = 0;
                    diag_reassoc!(
                        "upload_http: pre-connect selected candidate idx={} channel_hint={} bssid_hint={} candidate_count={}",
                        ap_candidate_idx,
                        candidate.hint.channel,
                        format_bssid(candidate.hint.bssid),
                        ap_candidates.len(),
                    );
                    Timer::after(Duration::from_millis(WIFI_SHORT_SETTLE_MS)).await;
                    continue;
                }
            }
        }

        // Reset stale disconnect state before this connect attempt so we do not
        // miss a fresh disconnect that races right after connect succeeds.
        WIFI_DISCONNECTED_EVENT.store(false, Ordering::Relaxed);
        net_attempt = net_attempt.saturating_add(1);
        ladder_step = RecoveryLadderStep::RetrySame;
        transition_state(
            &mut net_state,
            NetState::Associating,
            "connect_begin",
            started_at,
            ladder_step,
            net_attempt,
            (failure_class, failure_code),
        );
        publish_state(
            net_state,
            ladder_step,
            net_attempt,
            failure_class,
            failure_code,
            started_at.elapsed().as_millis() as u32,
        );
        telemetry::record_wifi_reassoc_connect_begin(
            auth_method_idx,
            channel_hint,
            channel_probe_idx,
        );
        telemetry::record_wifi_connect_attempt(channel_hint, auth_method_idx);
        let connect_started_at = Instant::now();
        match with_timeout(
            Duration::from_millis(runtime_policy.connect_timeout_ms as u64),
            controller.connect_async(),
        )
        .await
        {
            Ok(Ok(())) => {
                telemetry::record_wifi_connect_success();
                telemetry::record_wifi_reassoc_connect_success(elapsed_ms_u32(connect_started_at));
                hard_recover_watchdog_started_at = None;
                escalated_auth_sweep_attempts_left = 0;
                failure_class = NetFailureClass::None;
                failure_code = 0;
                diag_wifi!("upload_http: wifi connected");
                transition_state(
                    &mut net_state,
                    NetState::DhcpWait,
                    "connect_ok",
                    started_at,
                    ladder_step,
                    net_attempt,
                    (failure_class, failure_code),
                );
                publish_state(
                    net_state,
                    ladder_step,
                    net_attempt,
                    failure_class,
                    failure_code,
                    started_at.elapsed().as_millis() as u32,
                );
                let mut dhcp_lease_observed = has_ipv4_lease(&stack);
                let dhcp_wait_started_at = Instant::now();
                loop {
                    if !service_mode::upload_enabled() {
                        telemetry::set_wifi_link_connected(false);
                        telemetry::set_upload_http_listener(false, None);
                        let _ = controller.disconnect_async().await;
                        dhcp_lease_reacquire_attempts = 0;
                        other_disconnect_streak = 0;
                        hard_recover_watchdog_started_at = None;
                        escalated_auth_sweep_attempts_left = 0;
                        diag_wifi!("upload_http: upload mode off while connected");
                        break;
                    }

                    apply_pending_runtime_policy_updates(&mut runtime_policy);

                    let mut reconnect_due_to_credentials = false;
                    while let Ok(updated) = WIFI_CREDENTIALS_UPDATES.try_receive() {
                        if credentials == Some(updated) {
                            diag_wifi!("upload_http: wifi credentials unchanged while connected");
                            continue;
                        }
                        credentials = Some(updated);
                        config_applied = false;
                        auth_method_idx = 0;
                        channel_hint = None;
                        bssid_hint = None;
                        ap_candidates.clear();
                        ap_candidate_idx = 0;
                        dhcp_same_candidate_timeout_streak = 0;
                        dhcp_lease_reacquire_attempts = 0;
                        other_disconnect_streak = 0;
                        hard_recover_watchdog_started_at = None;
                        escalated_auth_sweep_attempts_left = 0;
                        channel_probe_idx = 0;
                        reconnect_due_to_credentials = true;
                    }
                    if reconnect_due_to_credentials {
                        diag_wifi!("upload_http: wifi credentials changed, reconnecting");
                        telemetry::record_wifi_reassoc_credentials_changed();
                        disconnect_with_timeout(&mut controller, "credentials_changed").await;
                        dhcp_lease_reacquire_attempts = 0;
                        other_disconnect_streak = 0;
                        hard_recover_watchdog_started_at = None;
                        escalated_auth_sweep_attempts_left = 0;
                        telemetry::set_wifi_link_connected(false);
                        telemetry::set_upload_http_listener(false, None);
                        break;
                    }

                    if WIFI_DISCONNECTED_EVENT.swap(false, Ordering::Relaxed) {
                        let disconnect_reason = WIFI_LAST_DISCONNECT_REASON.load(Ordering::Relaxed);
                        if disconnect_reason == WIFI_REASON_OTHER {
                            other_disconnect_streak = other_disconnect_streak.saturating_add(1);
                        } else if disconnect_reason != 0 {
                            other_disconnect_streak = 0;
                        }
                        telemetry::record_wifi_reassoc_disconnect_event(disconnect_reason);
                        dhcp_lease_reacquire_attempts = 0;
                        telemetry::set_wifi_link_connected(false);
                        telemetry::set_upload_http_listener(false, None);
                        disconnect_with_timeout(&mut controller, "connected_watchdog").await;
                        config_applied = false;
                        if disconnect_reason == WIFI_REASON_OTHER
                            && other_disconnect_streak >= WIFI_REASON_OTHER_HARD_RECOVER_STREAK
                        {
                            channel_hint = None;
                            bssid_hint = None;
                            ap_candidates.clear();
                            ap_candidate_idx = 0;
                            channel_probe_idx = 0;
                            auth_method_idx = 0;
                            dhcp_same_candidate_timeout_streak = 0;
                            dhcp_lease_reacquire_attempts = 0;
                            other_disconnect_streak = 0;
                            hard_recover_watchdog_started_at = Some(Instant::now());
                            diag_reassoc!(
                                "upload_http: reason=other streak reached {}; forcing hard wifi recovery (stop/start + full discovery reset)",
                                WIFI_REASON_OTHER_HARD_RECOVER_STREAK
                            );
                            Timer::after(Duration::from_millis(
                                runtime_policy.driver_restart_backoff_ms as u64,
                            ))
                            .await;
                            break;
                        }
                        if disconnect_reason == WIFI_REASON_OTHER
                            || disconnect_reason == WIFI_REASON_BEACON_TIMEOUT
                        {
                            // Recover from sticky post-disconnect states by forcing a fresh
                            // channel/auth walk instead of reusing potentially stale hints.
                            channel_hint = None;
                            bssid_hint = None;
                            ap_candidates.clear();
                            ap_candidate_idx = 0;
                            channel_probe_idx = 0;
                            auth_method_idx = (auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
                            dhcp_same_candidate_timeout_streak = 0;
                            diag_reassoc!(
                                "upload_http: disconnect reason={} -> forcing full reconnect sweep auth={:?}",
                                disconnect_reason,
                                WIFI_AUTH_METHODS[auth_method_idx]
                            );
                        }
                        diag_wifi!("upload_http: wifi disconnected");
                        Timer::after(Duration::from_millis(WIFI_POST_DISCONNECT_SETTLE_MS)).await;
                        break;
                    }

                    if !dhcp_lease_observed {
                        dhcp_lease_observed = has_ipv4_lease(&stack);
                        if dhcp_lease_observed {
                            transition_state(
                                &mut net_state,
                                NetState::ListenerWait,
                                "dhcp_ready",
                                started_at,
                                ladder_step,
                                net_attempt,
                                (failure_class, failure_code),
                            );
                            publish_state(
                                net_state,
                                ladder_step,
                                net_attempt,
                                failure_class,
                                failure_code,
                                started_at.elapsed().as_millis() as u32,
                            );
                            dhcp_same_candidate_timeout_streak = 0;
                            dhcp_lease_reacquire_attempts = 0;
                            other_disconnect_streak = 0;
                        } else {
                            let dhcp_timeout_ms = effective_dhcp_timeout_ms(
                                runtime_policy,
                                bssid_hint.is_some(),
                                dhcp_same_candidate_timeout_streak,
                            );
                            if dhcp_wait_started_at.elapsed().as_millis() >= dhcp_timeout_ms as u64
                            {
                                telemetry::record_wifi_reassoc_disconnect_event(
                                    WIFI_REASON_DHCP_NO_IPV4_STALL,
                                );
                                telemetry::record_wifi_watchdog_disconnect();
                                failure_class = NetFailureClass::DhcpNoIpv4;
                                failure_code = WIFI_REASON_DHCP_NO_IPV4_STALL;
                                ladder_step = RecoveryLadderStep::RetrySame;
                                transition_state(
                                    &mut net_state,
                                    NetState::Recovering,
                                    "dhcp_stall",
                                    started_at,
                                    ladder_step,
                                    net_attempt,
                                    (failure_class, failure_code),
                                );
                                publish_state(
                                    net_state,
                                    ladder_step,
                                    net_attempt,
                                    failure_class,
                                    failure_code,
                                    started_at.elapsed().as_millis() as u32,
                                );
                                if dhcp_lease_reacquire_attempts
                                    < WIFI_DHCP_LEASE_REACQUIRE_MAX_ATTEMPTS
                                {
                                    dhcp_lease_reacquire_attempts =
                                        dhcp_lease_reacquire_attempts.saturating_add(1);
                                    config_applied = false;
                                    diag_reassoc!(
                                        "upload_http: dhcp/no-ipv4 stall; lease reacquire attempt {}/{} auth={:?} channel_hint={:?} bssid_hint={}",
                                        dhcp_lease_reacquire_attempts,
                                        WIFI_DHCP_LEASE_REACQUIRE_MAX_ATTEMPTS,
                                        WIFI_AUTH_METHODS[auth_method_idx],
                                        channel_hint,
                                        format_bssid_opt(bssid_hint),
                                    );
                                    disconnect_with_timeout(
                                        &mut controller,
                                        "dhcp_lease_reacquire",
                                    )
                                    .await;
                                    telemetry::set_wifi_link_connected(false);
                                    telemetry::set_upload_http_listener(false, None);
                                    Timer::after(Duration::from_millis(
                                        WIFI_DHCP_LEASE_REACQUIRE_BACKOFF_MS,
                                    ))
                                    .await;
                                    break;
                                }

                                dhcp_lease_reacquire_attempts = 0;
                                let previous_bssid = bssid_hint;
                                if previous_bssid.is_some() {
                                    diag_wifi!(
                                        "upload_http: dhcp/no-ipv4 stall on pinned bssid after {}ms; clearing bssid hint and reconnecting",
                                        dhcp_timeout_ms
                                    );
                                } else {
                                    diag_wifi!(
                                        "upload_http: dhcp/no-ipv4 stall after {}ms; reconnecting and retrying scan/auth",
                                        dhcp_timeout_ms
                                    );
                                    channel_probe_idx = 0;
                                }
                                if let Some(next_candidate) = rotate_to_next_candidate(
                                    &ap_candidates,
                                    previous_bssid,
                                    &mut ap_candidate_idx,
                                ) {
                                    channel_hint = Some(next_candidate.hint.channel);
                                    bssid_hint = Some(next_candidate.hint.bssid);
                                    auth_method_idx = 0;
                                    config_applied = false;
                                    if previous_bssid == Some(next_candidate.hint.bssid) {
                                        dhcp_same_candidate_timeout_streak =
                                            dhcp_same_candidate_timeout_streak.saturating_add(1);
                                    } else {
                                        dhcp_same_candidate_timeout_streak = 0;
                                    }
                                    diag_reassoc!(
                                        "upload_http: dhcp/no-ipv4 stall candidate rotate idx={} channel_hint={} bssid_hint={} same_streak={} candidates={}",
                                        ap_candidate_idx,
                                        next_candidate.hint.channel,
                                        format_bssid(next_candidate.hint.bssid),
                                        dhcp_same_candidate_timeout_streak,
                                        ap_candidates.len(),
                                    );
                                } else {
                                    dhcp_same_candidate_timeout_streak =
                                        dhcp_same_candidate_timeout_streak.saturating_add(1);
                                    channel_hint = None;
                                    bssid_hint = None;
                                    auth_method_idx = 0;
                                    config_applied = false;
                                    channel_probe_idx = 0;
                                    diag_reassoc!(
                                        "upload_http: dhcp/no-ipv4 stall no candidate available; forcing fresh discovery streak={}",
                                        dhcp_same_candidate_timeout_streak,
                                    );
                                }
                                let _ = controller.disconnect_async().await;
                                if dhcp_same_candidate_timeout_streak
                                    >= WIFI_DHCP_SAME_CANDIDATE_RESTART_STREAK
                                {
                                    diag_reassoc!(
                                        "upload_http: dhcp/no-ipv4 stall streak={} reached; forcing wifi stop/start and full rescan",
                                        dhcp_same_candidate_timeout_streak,
                                    );
                                    let _ = controller.stop_async().await;
                                    ap_candidates.clear();
                                    ap_candidate_idx = 0;
                                    channel_hint = None;
                                    bssid_hint = None;
                                    auth_method_idx = 0;
                                    config_applied = false;
                                    channel_probe_idx = 0;
                                    dhcp_same_candidate_timeout_streak = 0;
                                }
                                telemetry::set_wifi_link_connected(false);
                                telemetry::set_upload_http_listener(false, None);
                                break;
                            }
                        }
                    }

                    if dhcp_lease_observed {
                        let listener_enabled = service_mode::upload_http_listener_enabled();
                        let snapshot = telemetry::snapshot();
                        let lease_ipv4 = stack_ipv4_lease(&stack);
                        if !listener_enabled {
                            telemetry::set_upload_http_listener(false, lease_ipv4);
                        }
                        if !listener_enabled && lease_ipv4.is_some() {
                            transition_state(
                                &mut net_state,
                                NetState::Ready,
                                "listener_bypass_ready",
                                started_at,
                                ladder_step,
                                net_attempt,
                                (failure_class, failure_code),
                            );
                            net_attempt = 0;
                            ladder_step = RecoveryLadderStep::RetrySame;
                            failure_class = NetFailureClass::None;
                            failure_code = 0;
                            publish_state(
                                net_state,
                                ladder_step,
                                net_attempt,
                                failure_class,
                                failure_code,
                                started_at.elapsed().as_millis() as u32,
                            );
                        } else if snapshot.upload_http_listening
                            && snapshot.upload_http_ipv4.is_some()
                        {
                            transition_state(
                                &mut net_state,
                                NetState::Ready,
                                "listener_ready",
                                started_at,
                                ladder_step,
                                net_attempt,
                                (failure_class, failure_code),
                            );
                            net_attempt = 0;
                            ladder_step = RecoveryLadderStep::RetrySame;
                            failure_class = NetFailureClass::None;
                            failure_code = 0;
                            publish_state(
                                net_state,
                                ladder_step,
                                net_attempt,
                                failure_class,
                                failure_code,
                                started_at.elapsed().as_millis() as u32,
                            );
                        } else if listener_enabled
                            && dhcp_wait_started_at.elapsed().as_millis()
                                >= runtime_policy.listener_timeout_ms as u64
                        {
                            failure_class = NetFailureClass::ListenerNotReady;
                            failure_code = 1;
                            ladder_step = RecoveryLadderStep::RetrySame;
                            transition_state(
                                &mut net_state,
                                NetState::Recovering,
                                "listener_timeout",
                                started_at,
                                ladder_step,
                                net_attempt,
                                (failure_class, failure_code),
                            );
                            publish_state(
                                net_state,
                                ladder_step,
                                net_attempt,
                                failure_class,
                                failure_code,
                                started_at.elapsed().as_millis() as u32,
                            );
                            disconnect_with_timeout(&mut controller, "listener_timeout").await;
                            telemetry::set_wifi_link_connected(false);
                            telemetry::set_upload_http_listener(false, None);
                            break;
                        }
                    }

                    Timer::after(Duration::from_millis(WIFI_CONNECTED_WATCHDOG_MS)).await;
                }
            }
            Ok(Err(err)) => {
                let disconnect_reason = WIFI_LAST_DISCONNECT_REASON.swap(0, Ordering::Relaxed);
                dhcp_lease_reacquire_attempts = 0;
                if disconnect_reason == WIFI_REASON_OTHER {
                    other_disconnect_streak = other_disconnect_streak.saturating_add(1);
                } else if disconnect_reason != 0 {
                    other_disconnect_streak = 0;
                }
                telemetry::record_wifi_connect_failure(disconnect_reason);
                telemetry::record_wifi_reassoc_connect_failure_detail(
                    disconnect_reason,
                    elapsed_ms_u32(connect_started_at),
                );
                failure_class = if is_auth_disconnect_reason(disconnect_reason) {
                    NetFailureClass::AuthReject
                } else if is_discovery_disconnect_reason(disconnect_reason) {
                    NetFailureClass::DiscoveryEmpty
                } else {
                    NetFailureClass::ConnectTimeout
                };
                failure_code = disconnect_reason;
                ladder_step = RecoveryLadderStep::RetrySame;
                transition_state(
                    &mut net_state,
                    NetState::Recovering,
                    "connect_err",
                    started_at,
                    ladder_step,
                    net_attempt,
                    (failure_class, failure_code),
                );
                publish_state(
                    net_state,
                    ladder_step,
                    net_attempt,
                    failure_class,
                    failure_code,
                    started_at.elapsed().as_millis() as u32,
                );
                telemetry::set_upload_http_listener(false, None);
                // ESP-IDF warns scan requests are ineffective while connect is in
                // progress. Force a bounded disconnect before any diagnostics scan.
                disconnect_with_timeout(&mut controller, "connect_err_presolve").await;
                let discovery_reason = is_discovery_disconnect_reason(disconnect_reason);
                let auth_reason = is_auth_disconnect_reason(disconnect_reason);
                let escalated_scan_active = escalated_auth_sweep_attempts_left > 0;
                let should_scan = escalated_scan_active
                    || discovery_reason
                    || channel_hint.is_none()
                    || channel_probe_idx.is_multiple_of(4);
                let mut observed_candidates =
                    heapless::Vec::<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>::new();
                let mut observed_ap = None;
                let mut observed_scan_nomem = false;
                if should_scan {
                    if let Ok(ssid) = core::str::from_utf8(&active.ssid[..active.ssid_len as usize])
                    {
                        let scan_outcome =
                            scan_target_candidates(&mut controller, ssid, runtime_policy).await;
                        observed_scan_nomem = scan_outcome.hit_nomem;
                        observed_candidates = scan_outcome.candidates;
                        observed_ap = observed_candidates.first().copied();
                    }
                }
                let auth_method = WIFI_AUTH_METHODS[auth_method_idx];
                diag_reassoc!(
                    "upload_http: wifi connect err={:?} auth={:?} channel_hint={:?} bssid_hint={} observed_channel={:?} observed_bssid={} reason={} (0x{:02x} {}) discovery_reason={} should_scan={} scan_nomem={} probe_idx={}",
                    err,
                    auth_method,
                    channel_hint,
                    format_bssid_opt(bssid_hint),
                    observed_ap.map(|ap| ap.hint),
                    format_bssid_opt(observed_ap.map(|ap| ap.hint.bssid)),
                    disconnect_reason,
                    disconnect_reason,
                    disconnect_reason_label(disconnect_reason),
                    discovery_reason,
                    should_scan,
                    observed_scan_nomem,
                    channel_probe_idx,
                );
                if observed_scan_nomem {
                    failure_class = NetFailureClass::Transport;
                    failure_code = WIFI_REASON_SCAN_NOMEM;
                    ladder_step = RecoveryLadderStep::DriverRestart;
                    transition_state(
                        &mut net_state,
                        NetState::Recovering,
                        "connect_err_scan_nomem",
                        started_at,
                        ladder_step,
                        net_attempt,
                        (failure_class, failure_code),
                    );
                    publish_state(
                        net_state,
                        ladder_step,
                        net_attempt,
                        failure_class,
                        failure_code,
                        started_at.elapsed().as_millis() as u32,
                    );
                    disconnect_and_stop_with_timeout(&mut controller, "connect_err_scan_nomem")
                        .await;
                    telemetry::set_wifi_link_connected(false);
                    telemetry::set_upload_http_listener(false, None);
                    channel_probe_idx = 0;
                    channel_hint = None;
                    bssid_hint = None;
                    ap_candidates.clear();
                    ap_candidate_idx = 0;
                    auth_method_idx = 0;
                    config_applied = false;
                    dhcp_same_candidate_timeout_streak = 0;
                    dhcp_lease_reacquire_attempts = 0;
                    other_disconnect_streak = 0;
                    if hard_recover_watchdog_started_at.is_none() {
                        hard_recover_watchdog_started_at = Some(Instant::now());
                    }
                    Timer::after(Duration::from_millis(WIFI_NOMEM_RECOVERY_BACKOFF_MS)).await;
                    continue;
                }
                if escalated_scan_active {
                    channel_probe_idx = 0;
                    channel_hint = None;
                    bssid_hint = None;
                    ap_candidates.clear();
                    ap_candidate_idx = 0;
                    config_applied = false;
                    dhcp_same_candidate_timeout_streak = 0;
                    if auth_reason {
                        auth_method_idx = (auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
                    } else {
                        auth_method_idx = 0;
                    }
                    escalated_auth_sweep_attempts_left =
                        escalated_auth_sweep_attempts_left.saturating_sub(1);
                    diag_reassoc!(
                        "upload_http: post-hard-recover-escalated-scan retry attempts_left={} auth={:?} reason={} ({})",
                        escalated_auth_sweep_attempts_left,
                        WIFI_AUTH_METHODS[auth_method_idx],
                        disconnect_reason,
                        disconnect_reason_label(disconnect_reason),
                    );
                    Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                    continue;
                }
                if disconnect_reason == WIFI_REASON_OTHER
                    && other_disconnect_streak >= WIFI_REASON_OTHER_HARD_RECOVER_STREAK
                {
                    channel_probe_idx = 0;
                    channel_hint = None;
                    bssid_hint = None;
                    ap_candidates.clear();
                    ap_candidate_idx = 0;
                    auth_method_idx = 0;
                    config_applied = false;
                    dhcp_same_candidate_timeout_streak = 0;
                    other_disconnect_streak = 0;
                    hard_recover_watchdog_started_at = Some(Instant::now());
                    diag_reassoc!(
                        "upload_http: connect reason=other streak reached {}; forcing hard wifi recovery (stop/start + full discovery reset)",
                        WIFI_REASON_OTHER_HARD_RECOVER_STREAK
                    );
                    Timer::after(Duration::from_millis(
                        runtime_policy.driver_restart_backoff_ms as u64,
                    ))
                    .await;
                    continue;
                }
                if let Some(ap) = observed_ap {
                    let mut selected_ap = ap;
                    let mut forced_rotation = false;
                    if disconnect_reason == WIFI_REASON_OTHER && observed_candidates.len() > 1 {
                        let rotate_from = bssid_hint.unwrap_or(ap.hint.bssid);
                        if let Some(next_candidate) = rotate_to_next_candidate(
                            &observed_candidates,
                            Some(rotate_from),
                            &mut ap_candidate_idx,
                        ) {
                            if next_candidate.hint.bssid != rotate_from {
                                selected_ap = next_candidate;
                                forced_rotation = true;
                                diag_reassoc!(
                                    "upload_http: reason=other; forcing candidate rotation idx={} channel_hint={} bssid_hint={} count={}",
                                    ap_candidate_idx,
                                    next_candidate.hint.channel,
                                    format_bssid(next_candidate.hint.bssid),
                                    observed_candidates.len(),
                                );
                            }
                        }
                    }
                    let selected_bssid = selected_ap.hint.bssid;
                    ap_candidate_idx = observed_candidates
                        .iter()
                        .position(|candidate| candidate.hint.bssid == selected_bssid)
                        .unwrap_or(0);
                    ap_candidates = observed_candidates;
                    if disconnect_reason == WIFI_REASON_OTHER && other_disconnect_streak >= 2 {
                        channel_hint = Some(selected_ap.hint.channel);
                        bssid_hint = None;
                        auth_method_idx = (auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
                        if other_disconnect_streak >= 2 + WIFI_AUTH_METHODS.len() as u8
                            && auth_method_idx == 0
                        {
                            channel_hint = None;
                            channel_probe_idx = 0;
                            ap_candidates.clear();
                            ap_candidate_idx = 0;
                            diag_reassoc!(
                                "upload_http: reason=other streak={} exhausted auth sweep; forcing full discovery",
                                other_disconnect_streak,
                            );
                        }
                        config_applied = false;
                        dhcp_same_candidate_timeout_streak = 0;
                        telemetry::record_wifi_reassoc_hint_retry(
                            channel_hint.unwrap_or(selected_ap.hint.channel),
                            auth_method_idx,
                            channel_probe_idx,
                        );
                        diag_reassoc!(
                            "upload_http: reason=other streak={}; dropping bssid pin for retry auth={:?} channel_hint={:?}",
                            other_disconnect_streak,
                            WIFI_AUTH_METHODS[auth_method_idx],
                            channel_hint,
                        );
                        Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                        continue;
                    }
                    if forced_rotation
                        || channel_hint != Some(selected_ap.hint.channel)
                        || bssid_hint != Some(selected_ap.hint.bssid)
                    {
                        channel_hint = Some(selected_ap.hint.channel);
                        bssid_hint = Some(selected_ap.hint.bssid);
                        auth_method_idx = 0;
                        config_applied = false;
                        dhcp_same_candidate_timeout_streak = 0;
                        telemetry::record_wifi_reassoc_hint_retry(
                            selected_ap.hint.channel,
                            auth_method_idx,
                            channel_probe_idx,
                        );
                        diag_reassoc!(
                            "upload_http: retrying with candidate idx={} channel_hint={} bssid_hint={} count={}",
                            ap_candidate_idx,
                            selected_ap.hint.channel,
                            format_bssid(selected_ap.hint.bssid),
                            ap_candidates.len(),
                        );
                        Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                        continue;
                    }
                    diag_reassoc!(
                        "upload_http: keeping discovered channel_hint={} bssid_hint={} for next auth attempt (candidate_count={})",
                        ap.hint.channel,
                        format_bssid(ap.hint.bssid),
                        ap_candidates.len(),
                    );
                }

                if discovery_reason {
                    if channel_probe_idx < WIFI_CHANNEL_PROBE_SEQUENCE.len() {
                        let next_channel = next_probe_channel(&mut channel_probe_idx);
                        channel_hint = Some(next_channel);
                        bssid_hint = None;
                        auth_method_idx = 0;
                        config_applied = false;
                        dhcp_same_candidate_timeout_streak = 0;
                        telemetry::record_wifi_reassoc_channel_probe(
                            next_channel,
                            channel_probe_idx,
                        );
                        diag_reassoc!(
                            "upload_http: discovery retry via channel probe channel_hint={} probe_idx={}",
                            next_channel,
                            channel_probe_idx
                        );
                        Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                        continue;
                    }
                    channel_probe_idx = 0;
                    channel_hint = None;
                    bssid_hint = None;
                    ap_candidates.clear();
                    ap_candidate_idx = 0;
                    auth_method_idx = 0;
                    config_applied = false;
                    dhcp_same_candidate_timeout_streak = 0;
                    diag_reassoc!(
                        "upload_http: discovery sweep exhausted; clearing hints for full rescan"
                    );
                    Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                    continue;
                }

                if auth_reason {
                    auth_method_idx = (auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
                    config_applied = false;
                    telemetry::record_wifi_reassoc_auth_rotation(
                        auth_method_idx,
                        channel_hint,
                        channel_probe_idx,
                    );
                    diag_reassoc!(
                        "upload_http: rotating auth on hinted channel auth={:?} channel_hint={:?} bssid_hint={}",
                        WIFI_AUTH_METHODS[auth_method_idx],
                        channel_hint,
                        format_bssid_opt(bssid_hint),
                    );
                    if auth_method_idx == 0 && channel_hint.is_some() {
                        if let Some(next_candidate) = rotate_to_next_candidate(
                            &ap_candidates,
                            bssid_hint,
                            &mut ap_candidate_idx,
                        ) {
                            channel_hint = Some(next_candidate.hint.channel);
                            bssid_hint = Some(next_candidate.hint.bssid);
                            config_applied = false;
                            dhcp_same_candidate_timeout_streak = 0;
                            diag_reassoc!(
                                "upload_http: auth methods exhausted; switching to next candidate idx={} channel_hint={} bssid_hint={}",
                                ap_candidate_idx,
                                next_candidate.hint.channel,
                                format_bssid(next_candidate.hint.bssid),
                            );
                        } else {
                            channel_hint = None;
                            bssid_hint = None;
                            channel_probe_idx = 0;
                            dhcp_same_candidate_timeout_streak = 0;
                            diag_reassoc!(
                                "upload_http: auth methods exhausted on hinted channel; clearing hints for discovery sweep",
                            );
                        }
                    }
                    Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
                    continue;
                }

                channel_probe_idx = 0;
                channel_hint = None;
                bssid_hint = None;
                ap_candidates.clear();
                ap_candidate_idx = 0;
                auth_method_idx = 0;
                config_applied = false;
                dhcp_same_candidate_timeout_streak = 0;
                diag_reassoc!(
                    "upload_http: reconnect fallback reason={} ({}); forcing fresh full scan",
                    disconnect_reason,
                    disconnect_reason_label(disconnect_reason)
                );
                Timer::after(Duration::from_millis(WIFI_RECOVERY_RETRY_BACKOFF_MS)).await;
            }
            Err(_) => {
                dhcp_lease_reacquire_attempts = 0;
                telemetry::record_wifi_connect_failure(WIFI_REASON_CONNECT_ATTEMPT_TIMEOUT);
                telemetry::record_wifi_reassoc_connect_failure_detail(
                    WIFI_REASON_CONNECT_ATTEMPT_TIMEOUT,
                    elapsed_ms_u32(connect_started_at),
                );
                failure_class = NetFailureClass::ConnectTimeout;
                failure_code = WIFI_REASON_CONNECT_ATTEMPT_TIMEOUT;
                ladder_step = RecoveryLadderStep::DriverRestart;
                transition_state(
                    &mut net_state,
                    NetState::Recovering,
                    "connect_timeout",
                    started_at,
                    ladder_step,
                    net_attempt,
                    (failure_class, failure_code),
                );
                publish_state(
                    net_state,
                    ladder_step,
                    net_attempt,
                    failure_class,
                    failure_code,
                    started_at.elapsed().as_millis() as u32,
                );
                telemetry::set_wifi_link_connected(false);
                telemetry::set_upload_http_listener(false, None);
                disconnect_and_stop_with_timeout(&mut controller, "connect_timeout").await;
                config_applied = false;
                channel_hint = None;
                bssid_hint = None;
                ap_candidates.clear();
                ap_candidate_idx = 0;
                channel_probe_idx = 0;
                auth_method_idx = 0;
                dhcp_same_candidate_timeout_streak = 0;
                other_disconnect_streak = 0;
                if hard_recover_watchdog_started_at.is_none() {
                    hard_recover_watchdog_started_at = Some(Instant::now());
                }
                diag_reassoc!(
                    "upload_http: wifi connect timeout after {}ms; forcing driver restart and full discovery reset",
                    runtime_policy.connect_timeout_ms
                );
                Timer::after(Duration::from_millis(
                    runtime_policy.driver_restart_backoff_ms as u64,
                ))
                .await;
                continue;
            }
        }
    }
}

fn apply_pending_runtime_policy_updates(runtime_policy: &mut WifiRuntimePolicy) {
    while let Ok(updated) = WIFI_RUNTIME_POLICY_UPDATES.try_receive() {
        let sanitized = updated.sanitized();
        if sanitized == *runtime_policy {
            continue;
        }
        *runtime_policy = sanitized;
        diag_wifi!(
            "upload_http: runtime wifi policy updated connect_timeout_ms={} dhcp_timeout_ms={} pinned_dhcp_timeout_ms={} listener_timeout_ms={}",
            runtime_policy.connect_timeout_ms,
            runtime_policy.dhcp_timeout_ms,
            runtime_policy.pinned_dhcp_timeout_ms,
            runtime_policy.listener_timeout_ms
        );
    }
}

fn transition_state(
    current: &mut NetState,
    next: NetState,
    trigger: &str,
    started_at: Instant,
    ladder_step: RecoveryLadderStep,
    net_attempt: u32,
    failure: (NetFailureClass, u8),
) {
    if *current == next {
        return;
    }
    task::emit_net_event(*current, next, trigger, started_at);
    if let Some(stage) = state_mem_stage(next) {
        log_radio_mem_diag_with_trigger(stage, trigger);
    }
    *current = next;
    publish_state(
        *current,
        ladder_step,
        net_attempt,
        failure.0,
        failure.1,
        started_at.elapsed().as_millis() as u32,
    );
}

fn state_mem_stage(state: NetState) -> Option<&'static str> {
    match state {
        NetState::Starting => Some("state_starting"),
        NetState::Scanning => Some("state_scanning"),
        NetState::Associating => Some("state_associating"),
        NetState::DhcpWait => Some("state_dhcp_wait"),
        NetState::ListenerWait => Some("state_listener_wait"),
        NetState::Ready => Some("state_ready"),
        NetState::Recovering => Some("state_recovering"),
        NetState::Idle | NetState::Failed => None,
    }
}

fn install_wifi_event_logger() {
    if WIFI_EVENT_LOGGER_INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }

    event::StaDisconnected::update_handler(|event| {
        let reason = event.reason();
        WIFI_LAST_DISCONNECT_REASON.store(reason, Ordering::Relaxed);
        WIFI_DISCONNECTED_EVENT.store(true, Ordering::Relaxed);
        if cfg!(debug_assertions) {
            diag_reassoc!(
                "upload_http: event sta_disconnected reason={} ({}) rssi={}",
                reason,
                disconnect_reason_label(reason),
                event.rssi()
            );
        }
    });

    if !cfg!(debug_assertions) {
        return;
    }

    event::StaStart::update_handler(|_| {
        diag_reassoc!("upload_http: event sta_start");
    });

    event::StaStop::update_handler(|_| {
        diag_reassoc!("upload_http: event sta_stop");
    });

    event::ScanDone::update_handler(|event| {
        diag_reassoc!(
            "upload_http: event scan_done status={} count={} scan_id={}",
            event.status(),
            event.number(),
            event.id()
        );
    });

    event::StaConnected::update_handler(|event| {
        let ssid_len = (event.ssid_len() as usize).min(event.ssid().len());
        let ssid = core::str::from_utf8(&event.ssid()[..ssid_len]).unwrap_or("<non_utf8>");
        let bssid = event.bssid();
        diag_reassoc!(
            "upload_http: event sta_connected ssid={} channel={} authmode={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            ssid,
            event.channel(),
            event.authmode(),
            bssid.first().copied().unwrap_or(0),
            bssid.get(1).copied().unwrap_or(0),
            bssid.get(2).copied().unwrap_or(0),
            bssid.get(3).copied().unwrap_or(0),
            bssid.get(4).copied().unwrap_or(0),
            bssid.get(5).copied().unwrap_or(0),
        );
    });
}

fn disconnect_reason_label(reason: u8) -> &'static str {
    match reason {
        WIFI_REASON_BEACON_TIMEOUT => "beacon_timeout",
        WIFI_REASON_NO_AP_FOUND => "no_ap_found",
        WIFI_REASON_AUTH_FAIL => "auth_fail",
        WIFI_REASON_ASSOC_FAIL => "assoc_fail",
        WIFI_REASON_HANDSHAKE_TIMEOUT => "handshake_timeout",
        WIFI_REASON_CONNECTION_FAIL => "connection_fail",
        WIFI_REASON_NO_AP_FOUND_COMPAT_SECURITY => "no_ap_found_compatible_security",
        WIFI_REASON_NO_AP_FOUND_AUTHMODE_THRESHOLD => "no_ap_found_authmode_threshold",
        WIFI_REASON_NO_AP_FOUND_RSSI_THRESHOLD => "no_ap_found_rssi_threshold",
        WIFI_REASON_DHCP_NO_IPV4_STALL => "dhcp_no_ipv4_stall",
        WIFI_REASON_POST_HARD_RECOVER_CONNECT_STALL => "post_hard_recover_connect_stall",
        WIFI_REASON_CONNECT_ATTEMPT_TIMEOUT => "connect_attempt_timeout",
        WIFI_REASON_START_NOMEM => "start_nomem",
        WIFI_REASON_SCAN_NOMEM => "scan_nomem",
        _ => "other",
    }
}

fn is_discovery_disconnect_reason(reason: u8) -> bool {
    reason == WIFI_REASON_BEACON_TIMEOUT
        || reason == WIFI_REASON_NO_AP_FOUND
        || reason == WIFI_REASON_NO_AP_FOUND_RSSI_THRESHOLD
}

fn is_auth_disconnect_reason(reason: u8) -> bool {
    reason == WIFI_REASON_AUTH_FAIL
        || reason == WIFI_REASON_ASSOC_FAIL
        || reason == WIFI_REASON_HANDSHAKE_TIMEOUT
        || reason == WIFI_REASON_CONNECTION_FAIL
        || reason == WIFI_REASON_NO_AP_FOUND_COMPAT_SECURITY
        || reason == WIFI_REASON_NO_AP_FOUND_AUTHMODE_THRESHOLD
}

fn next_probe_channel(index: &mut usize) -> u8 {
    let channel = WIFI_CHANNEL_PROBE_SEQUENCE[*index % WIFI_CHANNEL_PROBE_SEQUENCE.len()];
    *index = index.saturating_add(1);
    channel
}

fn wifi_credentials() -> Option<(&'static str, &'static str)> {
    let ssid = option_env!("MEDITAMER_WIFI_SSID").or(option_env!("SSID"))?;
    let password = option_env!("MEDITAMER_WIFI_PASSWORD")
        .or(option_env!("PASSWORD"))
        .unwrap_or("");
    Some((ssid, password))
}

fn wifi_credentials_from_parts(
    ssid: &[u8],
    password: &[u8],
) -> Result<WifiCredentials, &'static str> {
    if ssid.is_empty() || ssid.len() > WIFI_SSID_MAX || password.len() > WIFI_PASSWORD_MAX {
        return Err("invalid wifi credentials length");
    }
    let mut result = WifiCredentials {
        ssid: [0u8; WIFI_SSID_MAX],
        ssid_len: ssid.len() as u8,
        password: [0u8; WIFI_PASSWORD_MAX],
        password_len: password.len() as u8,
    };
    result.ssid[..ssid.len()].copy_from_slice(ssid);
    result.password[..password.len()].copy_from_slice(password);
    Ok(result)
}

fn mode_config_from_credentials(
    credentials: WifiCredentials,
    auth_method: AuthMethod,
    channel_hint: Option<u8>,
    bssid_hint: Option<[u8; 6]>,
) -> Option<ModeConfig> {
    let ssid = core::str::from_utf8(&credentials.ssid[..credentials.ssid_len as usize]).ok()?;
    let password =
        core::str::from_utf8(&credentials.password[..credentials.password_len as usize]).ok()?;
    let auth_method = if password.is_empty() {
        AuthMethod::None
    } else {
        auth_method
    };
    let scan_method = if channel_hint.is_some() {
        ScanMethod::Fast
    } else {
        ScanMethod::AllChannels
    };
    let mut client = ClientConfig::default()
        .with_ssid(ssid.into())
        .with_password(password.into())
        .with_auth_method(auth_method)
        .with_scan_method(scan_method);
    if let Some(channel) = channel_hint {
        client = client.with_channel(channel);
    }
    if let Some(bssid) = bssid_hint {
        client = client.with_bssid(bssid);
    }
    Some(ModeConfig::Client(client))
}

fn active_scan_timeout_ms(policy: WifiRuntimePolicy) -> u64 {
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

fn passive_scan_timeout_ms(policy: WifiRuntimePolicy) -> u64 {
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

fn post_recover_watchdog_timeout_ms(policy: WifiRuntimePolicy) -> u64 {
    // Watchdog budget intentionally covers at least one full discovery cycle
    // (active + passive + reconnect overhead) to avoid resetting recovery
    // state before channel/auth rotation can progress.
    // Keep this larger than one connect timeout so the connect API one-shot
    // semantics can be retried through the recovery ladder without premature reset.
    // Source (`esp_wifi_connect` single-attempt behavior): https://docs.espressif.com/projects/esp-idf/en/v5.3.1/esp32/api-reference/network/esp_wifi.html#_CPPv416esp_wifi_connectv
    let scan_budget_ms = active_scan_timeout_ms(policy)
        .saturating_add(passive_scan_timeout_ms(policy))
        .saturating_add(6_000);
    (policy.connect_timeout_ms as u64)
        .saturating_add(scan_budget_ms)
        .max((policy.connect_timeout_ms as u64).saturating_mul(2))
}

async fn disconnect_with_timeout(controller: &mut WifiController<'static>, context: &str) {
    log_radio_mem_diag_with_trigger("recover_disconnect_before", context);
    match with_timeout(
        Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
        controller.disconnect_async(),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            diag_reassoc!("upload_http: {} disconnect err={:?}", context, err);
        }
        Err(_) => {
            diag_reassoc!(
                "upload_http: {} disconnect timeout={}ms",
                context,
                WIFI_DRIVER_CONTROL_TIMEOUT_MS
            );
        }
    }
    log_radio_mem_diag_with_trigger("recover_disconnect_after", context);
}

async fn disconnect_and_stop_with_timeout(controller: &mut WifiController<'static>, context: &str) {
    disconnect_with_timeout(controller, context).await;
    let mut stop_attempt = 0u8;
    loop {
        log_radio_mem_diag_with_trigger("recover_stop_before", context);
        match with_timeout(
            Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
            controller.stop_async(),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                diag_reassoc!(
                    "upload_http: {} stop err={:?} attempt={}",
                    context,
                    err,
                    stop_attempt + 1
                );
            }
            Err(_) => {
                diag_reassoc!(
                    "upload_http: {} stop timeout={}ms attempt={}",
                    context,
                    WIFI_DRIVER_CONTROL_TIMEOUT_MS,
                    stop_attempt + 1
                );
            }
        }
        log_radio_mem_diag_with_trigger("recover_stop_after", context);
        match controller.is_started() {
            Ok(false) => break,
            Ok(true) => {
                if stop_attempt >= WIFI_DRIVER_STOP_RETRIES {
                    diag_reassoc!(
                        "upload_http: {} stop retries exhausted; controller still started",
                        context
                    );
                    break;
                }
            }
            Err(err) => {
                diag_reassoc!(
                    "upload_http: {} is_started check err={:?} after stop",
                    context,
                    err
                );
                break;
            }
        }
        stop_attempt = stop_attempt.saturating_add(1);
        Timer::after(Duration::from_millis(WIFI_DRIVER_STOP_RETRY_BACKOFF_MS)).await;
    }
}

async fn scan_target_candidates(
    controller: &mut WifiController<'static>,
    target_ssid: &str,
    runtime_policy: WifiRuntimePolicy,
) -> ScanOutcome {
    let mut candidates = heapless::Vec::<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>::new();
    let active_timeout_ms = active_scan_timeout_ms(runtime_policy);
    let directed_timeout_ms = active_timeout_ms.min(8_000).max(3_000);
    let passive_timeout_ms = passive_scan_timeout_ms(runtime_policy);
    let active_timeout = Duration::from_millis(active_timeout_ms);
    let directed_timeout = Duration::from_millis(directed_timeout_ms);
    let passive_timeout = Duration::from_millis(passive_timeout_ms);
    let probe_timeout_ms = active_timeout_ms.min(6_000).max(2_500);
    let probe_timeout = Duration::from_millis(probe_timeout_ms);
    let mut any_nonzero_results = false;

    let active = driver::active_scan_config(runtime_policy).with_max(WIFI_SCAN_DIAG_MAX_APS);
    log_radio_mem_diag("scan_active_broad_before");
    let active_started_at = Instant::now();
    match with_timeout(active_timeout, controller.scan_with_config_async(active)).await {
        Ok(Ok(results)) => {
            log_radio_mem_diag("scan_active_broad_ok");
            any_nonzero_results |= !results.is_empty();
            collect_scan_results("active_broad", target_ssid, &results, &mut candidates);
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Active,
                results.len(),
                !candidates.is_empty(),
                elapsed_ms_u32(active_started_at),
                candidates.first().map(|ap| ap.hint.channel),
            );
        }
        Ok(Err(err)) => {
            diag_reassoc!(
                "upload_http: scan active_broad err={:?} target_ssid={}",
                err,
                target_ssid
            );
            if is_no_mem_wifi_error(&err) {
                diag_reassoc!(
                    "upload_http: scan active_broad NoMem target_ssid={}",
                    target_ssid
                );
                log_radio_mem_diag("scan_active_broad_nomem");
                return ScanOutcome {
                    candidates,
                    hit_nomem: true,
                };
            }
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Active,
                0,
                false,
                elapsed_ms_u32(active_started_at),
                None,
            );
        }
        Err(_) => {
            diag_reassoc!(
                "upload_http: scan active_broad timeout={}ms target_ssid={}",
                active_timeout_ms,
                target_ssid
            );
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Active,
                0,
                false,
                elapsed_ms_u32(active_started_at),
                None,
            );
        }
    }

    if !candidates.is_empty() {
        diag_reassoc!(
            "upload_http: scan target_ssid={} candidate_count={} top_channel={} top_bssid={}",
            target_ssid,
            candidates.len(),
            candidates.first().map(|ap| ap.hint.channel).unwrap_or(0),
            format_bssid_opt(candidates.first().map(|ap| ap.hint.bssid)),
        );
        return ScanOutcome {
            candidates,
            hit_nomem: false,
        };
    }

    let directed = driver::directed_active_scan_config(target_ssid, runtime_policy)
        .with_max(WIFI_SCAN_DIAG_MAX_APS);
    log_radio_mem_diag("scan_active_directed_before");
    let directed_started_at = Instant::now();
    match with_timeout(
        directed_timeout,
        controller.scan_with_config_async(directed),
    )
    .await
    {
        Ok(Ok(results)) => {
            log_radio_mem_diag("scan_active_directed_ok");
            any_nonzero_results |= !results.is_empty();
            collect_scan_results("active_directed", target_ssid, &results, &mut candidates);
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Active,
                results.len(),
                !candidates.is_empty(),
                elapsed_ms_u32(directed_started_at),
                candidates.first().map(|ap| ap.hint.channel),
            );
        }
        Ok(Err(err)) => {
            diag_reassoc!(
                "upload_http: scan active_directed err={:?} target_ssid={}",
                err,
                target_ssid
            );
            if is_no_mem_wifi_error(&err) {
                diag_reassoc!(
                    "upload_http: scan active_directed NoMem target_ssid={}",
                    target_ssid
                );
                log_radio_mem_diag("scan_active_directed_nomem");
                return ScanOutcome {
                    candidates,
                    hit_nomem: true,
                };
            }
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Active,
                0,
                false,
                elapsed_ms_u32(directed_started_at),
                None,
            );
        }
        Err(_) => {
            diag_reassoc!(
                "upload_http: scan active_directed timeout={}ms target_ssid={}",
                directed_timeout_ms,
                target_ssid
            );
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Active,
                0,
                false,
                elapsed_ms_u32(directed_started_at),
                None,
            );
        }
    }
    if !candidates.is_empty() {
        diag_reassoc!(
            "upload_http: scan target_ssid={} candidate_count={} top_channel={} top_bssid={}",
            target_ssid,
            candidates.len(),
            candidates.first().map(|ap| ap.hint.channel).unwrap_or(0),
            format_bssid_opt(candidates.first().map(|ap| ap.hint.bssid)),
        );
        return ScanOutcome {
            candidates,
            hit_nomem: false,
        };
    }

    let passive = driver::passive_scan_config(runtime_policy).with_max(WIFI_SCAN_DIAG_MAX_APS);
    log_radio_mem_diag("scan_passive_before");
    let passive_started_at = Instant::now();
    match with_timeout(passive_timeout, controller.scan_with_config_async(passive)).await {
        Ok(Ok(results)) => {
            log_radio_mem_diag("scan_passive_ok");
            any_nonzero_results |= !results.is_empty();
            collect_scan_results("passive", target_ssid, &results, &mut candidates);
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Passive,
                results.len(),
                !candidates.is_empty(),
                elapsed_ms_u32(passive_started_at),
                candidates.first().map(|ap| ap.hint.channel),
            );
        }
        Ok(Err(err)) => {
            diag_reassoc!(
                "upload_http: scan passive err={:?} target_ssid={}",
                err,
                target_ssid
            );
            if is_no_mem_wifi_error(&err) {
                diag_reassoc!(
                    "upload_http: scan passive NoMem target_ssid={}",
                    target_ssid
                );
                log_radio_mem_diag("scan_passive_nomem");
                return ScanOutcome {
                    candidates,
                    hit_nomem: true,
                };
            }
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Passive,
                0,
                false,
                elapsed_ms_u32(passive_started_at),
                None,
            );
        }
        Err(_) => {
            diag_reassoc!(
                "upload_http: scan passive timeout={}ms target_ssid={}",
                passive_timeout_ms,
                target_ssid
            );
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Passive,
                0,
                false,
                elapsed_ms_u32(passive_started_at),
                None,
            );
        }
    }

    if !candidates.is_empty() {
        diag_reassoc!(
            "upload_http: scan target_ssid={} candidate_count={} top_channel={} top_bssid={}",
            target_ssid,
            candidates.len(),
            candidates.first().map(|ap| ap.hint.channel).unwrap_or(0),
            format_bssid_opt(candidates.first().map(|ap| ap.hint.bssid)),
        );
        return ScanOutcome {
            candidates,
            hit_nomem: false,
        };
    }

    if !any_nonzero_results {
        diag_reassoc!(
            "upload_http: scan zero_result_fallback start channels={:?} target_ssid={} probe_timeout_ms={}",
            WIFI_ZERO_DISCOVERY_SCAN_PROBE_CHANNELS,
            target_ssid,
            probe_timeout_ms,
        );
        for channel in WIFI_ZERO_DISCOVERY_SCAN_PROBE_CHANNELS {
            let probe = driver::channel_active_scan_config(channel, runtime_policy)
                .with_max(WIFI_SCAN_DIAG_MAX_APS);
            log_radio_mem_diag("scan_probe_before");
            let probe_started_at = Instant::now();
            match with_timeout(probe_timeout, controller.scan_with_config_async(probe)).await {
                Ok(Ok(results)) => {
                    log_radio_mem_diag("scan_probe_ok");
                    diag_reassoc!(
                        "upload_http: scan probe channel={} found={} target_ssid={}",
                        channel,
                        results.len(),
                        target_ssid
                    );
                    collect_scan_results("probe", target_ssid, &results, &mut candidates);
                    telemetry::record_wifi_reassoc_scan(
                        telemetry::WifiScanPhase::Active,
                        results.len(),
                        !candidates.is_empty(),
                        elapsed_ms_u32(probe_started_at),
                        candidates.first().map(|ap| ap.hint.channel),
                    );
                }
                Ok(Err(err)) => {
                    diag_reassoc!(
                        "upload_http: scan probe err={:?} channel={} target_ssid={}",
                        err,
                        channel,
                        target_ssid
                    );
                    if is_no_mem_wifi_error(&err) {
                        diag_reassoc!(
                            "upload_http: scan probe NoMem channel={} target_ssid={}",
                            channel,
                            target_ssid
                        );
                        log_radio_mem_diag("scan_probe_nomem");
                        return ScanOutcome {
                            candidates,
                            hit_nomem: true,
                        };
                    }
                    telemetry::record_wifi_reassoc_scan(
                        telemetry::WifiScanPhase::Active,
                        0,
                        false,
                        elapsed_ms_u32(probe_started_at),
                        None,
                    );
                }
                Err(_) => {
                    diag_reassoc!(
                        "upload_http: scan probe timeout={}ms channel={} target_ssid={}",
                        probe_timeout_ms,
                        channel,
                        target_ssid
                    );
                    telemetry::record_wifi_reassoc_scan(
                        telemetry::WifiScanPhase::Active,
                        0,
                        false,
                        elapsed_ms_u32(probe_started_at),
                        None,
                    );
                }
            }
            if !candidates.is_empty() {
                break;
            }
        }
    }

    if !candidates.is_empty() {
        diag_reassoc!(
            "upload_http: scan target_ssid={} candidate_count={} top_channel={} top_bssid={}",
            target_ssid,
            candidates.len(),
            candidates.first().map(|ap| ap.hint.channel).unwrap_or(0),
            format_bssid_opt(candidates.first().map(|ap| ap.hint.bssid)),
        );
        return ScanOutcome {
            candidates,
            hit_nomem: false,
        };
    }

    diag_reassoc!("upload_http: scan target_ssid={} found=0", target_ssid);
    ScanOutcome {
        candidates,
        hit_nomem: false,
    }
}

fn collect_scan_results(
    label: &str,
    target_ssid: &str,
    results: &[AccessPointInfo],
    candidates: &mut heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
) {
    if results.is_empty() {
        telemetry::record_wifi_scan(0, false);
        diag_reassoc!(
            "upload_http: scan {} found=0 target_ssid={}",
            label,
            target_ssid
        );
        return;
    }

    diag_reassoc!(
        "upload_http: scan {} found={} target_ssid={}",
        label,
        results.len(),
        target_ssid
    );

    for ap in results.iter() {
        diag_reassoc!(
            "upload_http: scan ap ssid={} channel={} bssid={} rssi={} auth={:?}",
            ap.ssid,
            ap.channel,
            format_bssid(ap.bssid),
            ap.signal_strength,
            ap.auth_method
        );
        if ap.ssid == target_ssid {
            insert_or_update_candidate(
                candidates,
                TargetApCandidate {
                    hint: TargetApHint {
                        channel: ap.channel,
                        bssid: ap.bssid,
                    },
                    rssi: ap.signal_strength,
                },
            );
        }
    }

    if let Some(ap) = candidates.first() {
        diag_reassoc!(
            "upload_http: scan target_ssid={} found_channel={} found_bssid={} via={} candidates={}",
            target_ssid,
            ap.hint.channel,
            format_bssid(ap.hint.bssid),
            label,
            candidates.len(),
        );
    }
    telemetry::record_wifi_scan(results.len(), !candidates.is_empty());
}

fn insert_or_update_candidate(
    candidates: &mut heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    candidate: TargetApCandidate,
) {
    if let Some(existing_idx) = candidates
        .iter()
        .position(|item| item.hint.bssid == candidate.hint.bssid)
    {
        if candidate.rssi > candidates[existing_idx].rssi {
            candidates[existing_idx] = candidate;
            sort_candidates_by_signal(candidates);
        }
        return;
    }
    if candidates.len() < WIFI_AP_CANDIDATE_MAX {
        let _ = candidates.push(candidate);
        sort_candidates_by_signal(candidates);
        return;
    }
    if let Some((weakest_idx, weakest)) = candidates
        .iter()
        .enumerate()
        .min_by_key(|(_, item)| item.rssi)
    {
        if candidate.rssi > weakest.rssi {
            candidates[weakest_idx] = candidate;
            sort_candidates_by_signal(candidates);
        }
    }
}

fn sort_candidates_by_signal(
    candidates: &mut heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
) {
    if candidates.len() < 2 {
        return;
    }
    let mut i = 1usize;
    while i < candidates.len() {
        let mut j = i;
        while j > 0 && candidates[j].rssi > candidates[j - 1].rssi {
            candidates.swap(j, j - 1);
            j -= 1;
        }
        i += 1;
    }
}

fn rotate_to_next_candidate(
    candidates: &heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    current_bssid: Option<[u8; 6]>,
    candidate_idx: &mut usize,
) -> Option<TargetApCandidate> {
    if candidates.is_empty() {
        return None;
    }
    if let Some(current_bssid) = current_bssid {
        if let Some(position) = candidates
            .iter()
            .position(|candidate| candidate.hint.bssid == current_bssid)
        {
            *candidate_idx = position;
        }
    } else {
        *candidate_idx = 0;
        return candidates.get(*candidate_idx).copied();
    }
    if candidates.len() > 1 {
        *candidate_idx = (*candidate_idx + 1) % candidates.len();
    } else {
        *candidate_idx = 0;
    }
    candidates.get(*candidate_idx).copied()
}

fn format_bssid(bssid: [u8; 6]) -> heapless::String<17> {
    let mut out = heapless::String::<17>::new();
    let _ = write!(
        out,
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        bssid[0], bssid[1], bssid[2], bssid[3], bssid[4], bssid[5]
    );
    out
}

fn format_bssid_opt(bssid: Option<[u8; 6]>) -> heapless::String<17> {
    match bssid {
        Some(value) => format_bssid(value),
        None => {
            let mut out = heapless::String::<17>::new();
            let _ = out.push_str("<none>");
            out
        }
    }
}

fn policy_total_attempt_budget(policy: WifiRuntimePolicy) -> u32 {
    u32::from(policy.retry_same_max)
        + u32::from(policy.rotate_candidate_max)
        + u32::from(policy.rotate_auth_max)
        + u32::from(policy.full_scan_reset_max)
        + u32::from(policy.driver_restart_max)
        + 1
}

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

fn is_no_mem_wifi_error(err: &WifiError) -> bool {
    matches!(err, WifiError::InternalError(InternalWifiError::NoMem))
}

fn log_radio_mem_diag(stage: &str) {
    log_radio_mem_diag_with_trigger(stage, "none");
}

fn log_radio_mem_diag_with_trigger(stage: &str, trigger: &str) {
    let snapshot = psram::allocator_memory_snapshot();
    diag_reassoc!(
        "upload_http: radio_mem stage={} trigger={} feature={} state={:?} total={} used={} free={} peak={} internal_free={} external_free={} min_free={} min_internal_free={} min_external_free={} large_alloc_external_ok={} large_alloc_internal_ok={} large_alloc_fail={}",
        stage,
        trigger,
        snapshot.feature_enabled,
        snapshot.state,
        snapshot.total_bytes,
        snapshot.used_bytes,
        snapshot.free_bytes,
        snapshot.peak_used_bytes,
        snapshot.free_internal_bytes,
        snapshot.free_external_bytes,
        snapshot.min_free_bytes,
        snapshot.min_free_internal_bytes,
        snapshot.min_free_external_bytes,
        snapshot.large_alloc_external_ok,
        snapshot.large_alloc_internal_ok,
        snapshot.large_alloc_fail
    );
}

fn stack_ipv4_lease(stack: &Stack<'static>) -> Option<[u8; 4]> {
    stack
        .config_v4()
        .map(|cfg| cfg.address.address().octets())
        .filter(|ip| *ip != [0, 0, 0, 0])
}

fn has_ipv4_lease(stack: &Stack<'static>) -> bool {
    stack_ipv4_lease(stack).is_some()
}
