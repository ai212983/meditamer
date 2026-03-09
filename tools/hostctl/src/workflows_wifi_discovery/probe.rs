use std::time::Instant;

use crate::workflows_wifi_common::{
    is_ready, parse_net_status_line, parse_scan_done_count, MemDiagSummary, PanicClass, PanicSignal,
};

#[derive(Clone, Debug)]
pub(super) struct RoundSample {
    pub round: u32,
    pub ready: bool,
    pub zero_discovery: bool,
    pub scan_zero_events: u32,
    pub scan_nonzero_events: u32,
    pub scan_runs_delta: u32,
    pub no_ap_found_events: u32,
    pub ssid_seen_events: u32,
    pub failure_class: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct WifiMetricsScanCounters {
    pub scan_runs: u32,
    pub scan_empty: u32,
    pub scan_hits: u32,
    pub no_ap_found: u32,
}

pub(super) struct ProbeRoundState {
    pub ready: bool,
    pub scan_zero_events: u32,
    pub scan_nonzero_events: u32,
    pub no_ap_found_events: u32,
    pub ssid_seen_events: u32,
    pub last_failure_class: String,
    pub read_mark: usize,
    pub next_status_poll: Instant,
    pub deadline: Instant,
    pub round_mem_diag: MemDiagSummary,
    pub panic_signal: Option<PanicSignal>,
    pub expected_soft_reset_armed: bool,
    pub expected_soft_reset_reboot_seen: bool,
    pub expected_soft_reset_recoveries: u32,
}

impl Default for ProbeRoundState {
    fn default() -> Self {
        Self {
            ready: false,
            scan_zero_events: 0,
            scan_nonzero_events: 0,
            no_ap_found_events: 0,
            ssid_seen_events: 0,
            last_failure_class: String::new(),
            read_mark: 0,
            next_status_poll: Instant::now(),
            deadline: Instant::now(),
            round_mem_diag: MemDiagSummary::default(),
            panic_signal: None,
            expected_soft_reset_armed: false,
            expected_soft_reset_reboot_seen: false,
            expected_soft_reset_recoveries: 0,
        }
    }
}

impl ProbeRoundState {
    pub(super) fn new(read_mark: usize, deadline: Instant) -> Self {
        Self {
            read_mark,
            next_status_poll: Instant::now(),
            deadline,
            last_failure_class: "none".to_string(),
            ..Self::default()
        }
    }

    pub(super) fn ingest_line(&mut self, line: &str, ssid_marker: &str, require_listener: bool) {
        if let Some(count) = parse_scan_done_count(line) {
            if count == 0 {
                self.scan_zero_events = self.scan_zero_events.saturating_add(1);
            } else {
                self.scan_nonzero_events = self.scan_nonzero_events.saturating_add(1);
            }
        }
        if line.contains("reason=201") || line.contains("no_ap_found") {
            self.no_ap_found_events = self.no_ap_found_events.saturating_add(1);
        }
        if line.starts_with("upload_http: scan ap ssid=") && line.contains(ssid_marker) {
            self.ssid_seen_events = self.ssid_seen_events.saturating_add(1);
        }
        if line.starts_with("NET_STATUS ") {
            if let Ok(status) = parse_net_status_line(line) {
                self.last_failure_class = status
                    .failure_class
                    .as_deref()
                    .unwrap_or("none")
                    .to_string();
                if is_ready(&status, require_listener) {
                    self.ready = true;
                }
            }
        }
        if line.contains("zero_discovery_hard_guard software_reset=true")
            || line.contains("zero_discovery_terminal software_reset=true")
            || line.contains("post_start_promisc_diag software_reset=true")
        {
            self.expected_soft_reset_armed = true;
        }
    }

    pub(super) fn is_zero_discovery(&self) -> bool {
        !self.ready && self.scan_zero_events > 0 && self.scan_nonzero_events == 0
    }

    pub(super) fn should_suppress_expected_soft_reset_reboot(
        &mut self,
        signal: &PanicSignal,
    ) -> bool {
        if signal.class != PanicClass::UnexpectedReboot || !self.expected_soft_reset_armed {
            return false;
        }
        self.expected_soft_reset_armed = false;
        self.expected_soft_reset_reboot_seen = true;
        true
    }
}
