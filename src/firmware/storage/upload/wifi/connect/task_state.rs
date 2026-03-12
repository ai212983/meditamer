use super::*;

pub(super) enum ConnectionAttempt {
    Continue,
    Proceed(WifiCredentials),
}

pub(super) struct WifiTaskState {
    pub(super) credentials: Option<WifiCredentials>,
    pub(super) runtime_policy: WifiRuntimePolicy,
    pub(super) net_state: NetState,
    pub(super) ladder_step: RecoveryLadderStep,
    pub(super) net_attempt: u32,
    pub(super) failure_class: NetFailureClass,
    pub(super) failure_code: u8,
    pub(super) config_applied: bool,
    pub(super) auth_method_idx: usize,
    pub(super) paused: bool,
    pub(super) channel_hint: Option<u8>,
    pub(super) bssid_hint: Option<[u8; 6]>,
    pub(super) ap_candidates: heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    pub(super) ap_candidate_idx: usize,
    pub(super) dhcp_same_candidate_timeout_streak: u8,
    pub(super) dhcp_lease_reacquire_attempts: u8,
    pub(super) listener_timeout_streak: u8,
    pub(super) listener_timeout_streak_start_internal_free: u32,
    pub(super) other_disconnect_streak: u8,
    pub(super) discovery_sweep_exhausted_streak: u8,
    pub(super) zero_discovery_hard_guard_restarts: u8,
    pub(super) force_full_channel_probe_next_scan: bool,
    pub(super) channel_probe_idx: usize,
    pub(super) hard_recover_watchdog_started_at: Option<Instant>,
    pub(super) hard_recover_watchdog_start_reason: &'static str,
    pub(super) hard_recover_watchdog_scan_rounds: u16,
    pub(super) hard_recover_watchdog_zero_scan_rounds: u16,
    pub(super) hard_recover_watchdog_connect_begins: u16,
    pub(super) hard_recover_watchdog_last_scan_completed_at: Option<Instant>,
    pub(super) hard_recover_watchdog_last_connect_begin_at: Option<Instant>,
    pub(super) start_attempt_started_at: Option<Instant>,
    pub(super) start_ok_at: Option<Instant>,
    pub(super) escalated_auth_sweep_attempts_left: u8,
    pub(super) terminal_fail_latched: bool,
    pub(super) started_at: Instant,
}

impl WifiTaskState {
    pub(super) fn new(credentials: Option<WifiCredentials>, started_at: Instant) -> Self {
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

    pub(super) fn point_age_ms(point: Option<Instant>) -> i64 {
        point
            .map(|instant| instant.elapsed().as_millis() as i64)
            .unwrap_or(-1)
    }

    pub(super) fn start_hard_recover_watchdog(&mut self, reason: &'static str) {
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

    pub(super) fn clear_hard_recover_watchdog(&mut self, reason: &'static str) {
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

    pub(super) fn note_hard_recover_scan_completion(
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

    pub(super) fn note_hard_recover_connect_begin(&mut self) {
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
