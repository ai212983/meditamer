use super::*;
mod attempt;
mod blob_state_diag;
mod boot_scan_diag;
mod boot_scan_idf_compare;
mod config;
mod driver_state;
mod error;
mod events;
mod idf_log_diag;
mod idf_scan_compare;
mod prepare;
mod promisc_diag;
mod recovery;
mod state_machine;
mod success;
mod timeout;
mod timing;
use attempt::perform_connect_attempt;
use boot_scan_diag::maybe_run_boot_scan_only_diag;
use config::mode_config_from_credentials;
use driver_state::{
    log_boot_scan_only_driver_state, maybe_log_first_start_driver_state,
    maybe_log_pre_start_driver_state, maybe_log_scan_entry_driver_state,
};
use error::handle_connect_error;
use events::{
    disconnect_reason_label, install_wifi_event_logger, is_auth_disconnect_reason,
    is_discovery_disconnect_reason, next_probe_channel, state_mem_stage,
};
use idf_log_diag::{maybe_begin_first_start_idf_log_diag, maybe_end_first_start_idf_log_diag};
use idf_scan_compare::maybe_run_scan_entry_idf_compare_diag;
use prepare::prepare_connection_attempt;
use promisc_diag::{
    maybe_handle_post_start_promisc_diag, maybe_run_boot_scan_only_promisc_diag,
    maybe_run_scan_entry_promisc_diag,
};
use recovery::{
    disconnect_and_stop_with_timeout, disconnect_with_timeout,
    maybe_software_reset_on_zero_discovery_hard_guard,
    maybe_software_reset_on_zero_discovery_terminal,
};
use state_machine::{apply_pending_runtime_policy_updates, transition_state};
use success::handle_connect_success;
use timeout::handle_connect_timeout;

pub(crate) use boot_scan_diag::boot_scan_only_diag_enabled;
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
    maybe_run_boot_scan_only_diag(&mut controller, state.credentials.is_some()).await;
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

pub(super) fn monotonic_now_ms_u32() -> u32 {
    Instant::now().as_millis() as u32
}

pub(super) fn tick_age_ms_u32(last_tick_ms: u32) -> i64 {
    if last_tick_ms == 0 {
        return -1;
    }
    let now = monotonic_now_ms_u32();
    i64::from(now.wrapping_sub(last_tick_ms))
}

const WIFI_REAPPLY_PROTOCOL_AFTER_START: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_REAPPLY_PROTOCOL_AFTER_START") {
        Some(value) => Some(value),
        None => option_env!("WIFI_REAPPLY_PROTOCOL_AFTER_START"),
    },
);
const WIFI_C_LIKE_DISCOVERY_START: bool =
    parse_nonzero_flag(match option_env!("MEDITAMER_WIFI_C_LIKE_DISCOVERY_START") {
        Some(value) => Some(value),
        None => option_env!("WIFI_C_LIKE_DISCOVERY_START"),
    });

pub(super) fn maybe_reapply_sta_protocol_after_start(controller: &mut WifiController<'static>) {
    if !WIFI_REAPPLY_PROTOCOL_AFTER_START {
        return;
    }
    let protocols = wifi_standard_bgn_protocols();
    match wifi_set_protocol(controller, protocols) {
        Ok(()) => diag_reassoc!("upload_http: post_start_protocol_reapply result=ok profile=bgn"),
        Err(err) => diag_reassoc!(
            "upload_http: post_start_protocol_reapply result=err profile=bgn err={:?}",
            err,
        ),
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
    listener_timeout_streak: u8,
    listener_timeout_streak_start_internal_free: u32,
    other_disconnect_streak: u8,
    discovery_sweep_exhausted_streak: u8,
    zero_discovery_hard_guard_restarts: u8,
    force_full_channel_probe_next_scan: bool,
    channel_probe_idx: usize,
    hard_recover_watchdog_started_at: Option<Instant>,
    hard_recover_watchdog_start_reason: &'static str,
    hard_recover_watchdog_scan_rounds: u16,
    hard_recover_watchdog_zero_scan_rounds: u16,
    hard_recover_watchdog_connect_begins: u16,
    hard_recover_watchdog_last_scan_completed_at: Option<Instant>,
    hard_recover_watchdog_last_connect_begin_at: Option<Instant>,
    start_attempt_started_at: Option<Instant>,
    start_ok_at: Option<Instant>,
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
            listener_timeout_streak: 0u8,
            listener_timeout_streak_start_internal_free: 0u32,
            other_disconnect_streak: 0u8,
            discovery_sweep_exhausted_streak: 0u8,
            zero_discovery_hard_guard_restarts: 0u8,
            force_full_channel_probe_next_scan: false,
            channel_probe_idx: 0usize,
            hard_recover_watchdog_started_at: None,
            hard_recover_watchdog_start_reason: "none",
            hard_recover_watchdog_scan_rounds: 0u16,
            hard_recover_watchdog_zero_scan_rounds: 0u16,
            hard_recover_watchdog_connect_begins: 0u16,
            hard_recover_watchdog_last_scan_completed_at: None,
            hard_recover_watchdog_last_connect_begin_at: None,
            start_attempt_started_at: None,
            start_ok_at: None,
            escalated_auth_sweep_attempts_left: 0u8,
            terminal_fail_latched: false,
            started_at,
        }
    }

    fn point_age_ms(point: Option<Instant>) -> i64 {
        point
            .map(|instant| instant.elapsed().as_millis() as i64)
            .unwrap_or(-1)
    }

    fn start_hard_recover_watchdog(&mut self, reason: &'static str) {
        if self.hard_recover_watchdog_started_at.is_some() {
            return;
        }
        self.hard_recover_watchdog_started_at = Some(Instant::now());
        self.hard_recover_watchdog_start_reason = reason;
        self.hard_recover_watchdog_scan_rounds = 0;
        self.hard_recover_watchdog_zero_scan_rounds = 0;
        self.hard_recover_watchdog_connect_begins = 0;
        self.hard_recover_watchdog_last_scan_completed_at = None;
        self.hard_recover_watchdog_last_connect_begin_at = None;
        diag_reassoc!(
            "upload_http: post-hard-recover-watchdog start reason={} timeout_ms={} connect_timeout_ms={}",
            reason,
            post_recover_watchdog_timeout_ms(self.runtime_policy),
            self.runtime_policy.connect_timeout_ms
        );
    }

    fn clear_hard_recover_watchdog(&mut self, reason: &'static str) {
        if let Some(started_at) = self.hard_recover_watchdog_started_at.take() {
            let elapsed_ms = started_at.elapsed().as_millis() as u64;
            let last_scan_age_ms =
                Self::point_age_ms(self.hard_recover_watchdog_last_scan_completed_at);
            let last_connect_begin_age_ms =
                Self::point_age_ms(self.hard_recover_watchdog_last_connect_begin_at);
            diag_reassoc!(
                "upload_http: post-hard-recover-watchdog clear reason={} start_reason={} elapsed_ms={} scan_rounds={} zero_scan_rounds={} connect_begins={} last_scan_age_ms={} last_connect_begin_age_ms={}",
                reason,
                self.hard_recover_watchdog_start_reason,
                elapsed_ms,
                self.hard_recover_watchdog_scan_rounds,
                self.hard_recover_watchdog_zero_scan_rounds,
                self.hard_recover_watchdog_connect_begins,
                last_scan_age_ms,
                last_connect_begin_age_ms,
            );
        }
        self.hard_recover_watchdog_start_reason = "none";
        self.hard_recover_watchdog_scan_rounds = 0;
        self.hard_recover_watchdog_zero_scan_rounds = 0;
        self.hard_recover_watchdog_connect_begins = 0;
        self.hard_recover_watchdog_last_scan_completed_at = None;
        self.hard_recover_watchdog_last_connect_begin_at = None;
    }

    fn note_hard_recover_scan_completion(
        &mut self,
        context: &'static str,
        saw_nonzero: bool,
        saw_target_candidate: bool,
    ) {
        if self.hard_recover_watchdog_started_at.is_none() {
            return;
        }
        self.hard_recover_watchdog_scan_rounds =
            self.hard_recover_watchdog_scan_rounds.saturating_add(1);
        if !saw_nonzero {
            self.hard_recover_watchdog_zero_scan_rounds = self
                .hard_recover_watchdog_zero_scan_rounds
                .saturating_add(1);
        }
        self.hard_recover_watchdog_last_scan_completed_at = Some(Instant::now());
        diag_reassoc!(
            "upload_http: post-hard-recover-watchdog scan_complete context={} start_reason={} scan_rounds={} zero_scan_rounds={} connect_begins={} saw_nonzero={} saw_target={}",
            context,
            self.hard_recover_watchdog_start_reason,
            self.hard_recover_watchdog_scan_rounds,
            self.hard_recover_watchdog_zero_scan_rounds,
            self.hard_recover_watchdog_connect_begins,
            saw_nonzero,
            saw_target_candidate,
        );
    }

    fn note_hard_recover_connect_begin(&mut self) {
        if self.hard_recover_watchdog_started_at.is_none() {
            return;
        }
        self.hard_recover_watchdog_connect_begins =
            self.hard_recover_watchdog_connect_begins.saturating_add(1);
        self.hard_recover_watchdog_last_connect_begin_at = Some(Instant::now());
        diag_reassoc!(
            "upload_http: post-hard-recover-watchdog connect_begin start_reason={} scan_rounds={} zero_scan_rounds={} connect_begins={}",
            self.hard_recover_watchdog_start_reason,
            self.hard_recover_watchdog_scan_rounds,
            self.hard_recover_watchdog_zero_scan_rounds,
            self.hard_recover_watchdog_connect_begins,
        );
    }
}
