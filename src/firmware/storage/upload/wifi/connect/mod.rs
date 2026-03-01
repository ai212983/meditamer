use super::*;

mod attempt;
mod config;
mod error;
mod events;
mod prepare;
mod recovery;
mod state_machine;
mod success;
mod timeout;
mod timing;

use attempt::perform_connect_attempt;
use config::mode_config_from_credentials;
use error::handle_connect_error;
use events::{
    disconnect_reason_label, install_wifi_event_logger, is_auth_disconnect_reason,
    is_discovery_disconnect_reason, next_probe_channel, state_mem_stage,
};
use prepare::prepare_connection_attempt;
use recovery::{disconnect_and_stop_with_timeout, disconnect_with_timeout};
use state_machine::{apply_pending_runtime_policy_updates, transition_state};
use success::handle_connect_success;
use timeout::handle_connect_timeout;

pub(super) use config::{wifi_credentials, wifi_credentials_from_parts};
pub(super) use timing::{
    active_scan_timeout_ms, directed_scan_timeout_ms, passive_scan_timeout_ms,
    post_recover_watchdog_timeout_ms, zero_discovery_probe_timeout_ms,
};

pub(super) async fn run_wifi_connection_task(
    mut controller: WifiController<'static>,
    _credentials: Option<WifiCredentials>,
    stack: Stack<'static>,
) {
    install_wifi_event_logger();
    telemetry::set_wifi_link_connected(false);
    let started_at = Instant::now();
    let mut state = WifiTaskState::new(_credentials, started_at);

    publish_config(state.credentials, state.runtime_policy);
    publish_state(
        state.net_state,
        state.ladder_step,
        state.net_attempt,
        state.failure_class,
        state.failure_code,
        state.started_at.elapsed().as_millis() as u32,
    );
    if state.credentials.is_none() {
        diag_wifi!("upload_http: waiting for NETCFG credentials over UART");
    }

    loop {
        let active = match prepare_connection_attempt(&mut controller, &mut state).await {
            ConnectionAttempt::Continue => continue,
            ConnectionAttempt::Proceed(active) => active,
        };
        perform_connect_attempt(&mut controller, &stack, &mut state).await;
        state.credentials = Some(active);
    }
}

enum ConnectionAttempt {
    Continue,
    Proceed(WifiCredentials),
}

struct WifiTaskState {
    credentials: Option<WifiCredentials>,
    runtime_policy: WifiRuntimePolicy,
    net_state: NetState,
    ladder_step: RecoveryLadderStep,
    net_attempt: u32,
    failure_class: NetFailureClass,
    failure_code: u8,
    config_applied: bool,
    auth_method_idx: usize,
    paused: bool,
    channel_hint: Option<u8>,
    bssid_hint: Option<[u8; 6]>,
    ap_candidates: heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    ap_candidate_idx: usize,
    dhcp_same_candidate_timeout_streak: u8,
    dhcp_lease_reacquire_attempts: u8,
    other_disconnect_streak: u8,
    discovery_sweep_exhausted_streak: u8,
    zero_discovery_hard_guard_restarts: u8,
    force_full_channel_probe_next_scan: bool,
    channel_probe_idx: usize,
    hard_recover_watchdog_started_at: Option<Instant>,
    escalated_auth_sweep_attempts_left: u8,
    terminal_fail_latched: bool,
    started_at: Instant,
}

impl WifiTaskState {
    fn new(credentials: Option<WifiCredentials>, started_at: Instant) -> Self {
        Self {
            credentials,
            runtime_policy: WifiRuntimePolicy::defaults().sanitized(),
            net_state: NetState::Idle,
            ladder_step: RecoveryLadderStep::RetrySame,
            net_attempt: 0u32,
            failure_class: NetFailureClass::None,
            failure_code: 0,
            config_applied: false,
            auth_method_idx: 0usize,
            paused: false,
            channel_hint: None,
            bssid_hint: None,
            ap_candidates: heapless::Vec::<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>::new(),
            ap_candidate_idx: 0usize,
            dhcp_same_candidate_timeout_streak: 0u8,
            dhcp_lease_reacquire_attempts: 0u8,
            other_disconnect_streak: 0u8,
            discovery_sweep_exhausted_streak: 0u8,
            zero_discovery_hard_guard_restarts: 0u8,
            force_full_channel_probe_next_scan: false,
            channel_probe_idx: 0usize,
            hard_recover_watchdog_started_at: None,
            escalated_auth_sweep_attempts_left: 0u8,
            terminal_fail_latched: false,
            started_at,
        }
    }
}
