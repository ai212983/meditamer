use super::super::{
    probe::ProbeRoundState, profile::recommended_round_timeout_ms, WifiDiscoveryRuntime,
};
use crate::workflows::wifi::common::{
    ctx_get_u32, detect_panic_signal, netcfg_set_payload, wait_net_ack,
};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::thread;
use std::time::{Duration, Instant};

const MAX_EXPECTED_SOFT_RESET_RECOVERIES_PER_ROUND: u32 = 1;
const FORCE_STOP_SETTLE_MS: u64 = 300;

impl WifiDiscoveryRuntime<'_> {
    pub(super) fn handle_probe_round(&mut self, context: &mut Value) -> Result<Value> {
        let round = ctx_get_u32(context, "round_index")?.saturating_add(1);
        let ssid_marker = format!("scan ap ssid={}", self.ssid);
        let require_listener = !self.profile.disable_listener_during_probe_rounds;
        let round_read_mark = self.console.mark();

        self.maybe_force_stop_before_round(round)?;
        self.maybe_recover_before_round(round)?;
        self.prepare_probe_round(round)?;
        self.run_control_command_with_guard(round, "NET START", 0)?;

        let mut state = self.new_probe_round_state(round_read_mark);
        while Instant::now() < state.deadline {
            state = self.collect_probe_round_lines(state, &ssid_marker, require_listener)?;
            if state.panic_signal.is_some() {
                break;
            }
            if state.ready {
                break;
            }
            self.send_status_poll_if_ready(&mut state);
            thread::sleep(Duration::from_millis(self.profile.poll_interval_ms as u64));
        }

        if let Some(signal) = state.panic_signal.clone() {
            if self.panic_first.is_none() {
                self.panic_first = Some(signal.clone());
            }
            if self.profile.disable_listener_during_probe_rounds {
                let _ = wait_net_ack(&mut self.console, "NET LISTENER ON");
            }
            let detail = self.panic_signal_detail(&signal);
            return Err(anyhow!("{detail}"));
        }

        let zero_discovery = state.is_zero_discovery();
        self.record_round_results(round, &state, zero_discovery);
        if self.profile.recover_after_round {
            if let Err(err) = self.maybe_recover_after_round(round) {
                self.logger.info(format!(
                    "round {round}: NET RECOVER after probe failed ({err})"
                ));
            }
        }
        Ok(serde_json::json!({
            "round_ready": state.ready,
            "round_zero_discovery": zero_discovery
        }))
    }

    fn maybe_recover_before_round(&mut self, round: u32) -> Result<()> {
        if !self.profile.recover_before_round {
            return Ok(());
        }
        self.run_control_command_with_guard(
            round,
            "NET RECOVER",
            self.profile.recover_settle_ms as u64,
        )
    }

    fn maybe_force_stop_before_round(&mut self, round: u32) -> Result<()> {
        if !self.profile.force_stop_before_round {
            return Ok(());
        }
        self.run_control_command_with_guard(round, "NET STOP", FORCE_STOP_SETTLE_MS)
    }

    fn prepare_probe_round(&mut self, round: u32) -> Result<()> {
        if !self.profile.disable_listener_during_probe_rounds {
            return Ok(());
        }
        self.run_control_command_with_guard(round, "NET LISTENER OFF", 0)
    }

    fn new_probe_round_state(&self, read_mark: usize) -> ProbeRoundState {
        ProbeRoundState::new(
            read_mark,
            Instant::now()
                + Duration::from_millis(recommended_round_timeout_ms(&self.policy, &self.profile)),
        )
    }

    fn collect_probe_round_lines(
        &mut self,
        mut state: ProbeRoundState,
        ssid_marker: &str,
        require_listener: bool,
    ) -> Result<ProbeRoundState> {
        self.console.poll_once()?;
        for line in self.console.read_recent_lines(state.read_mark) {
            let line_index = state.read_mark;
            state.read_mark = state.read_mark.saturating_add(1);
            self.mem_diag.record_line(&line);
            state.round_mem_diag.record_line(&line);
            if state.panic_signal.is_none() {
                if let Some(signal) = detect_panic_signal(&line, line_index) {
                    if state.should_suppress_expected_soft_reset_reboot(&signal)
                        && state.expected_soft_reset_recoveries
                            < MAX_EXPECTED_SOFT_RESET_RECOVERIES_PER_ROUND
                    {
                        state.expected_soft_reset_recoveries =
                            state.expected_soft_reset_recoveries.saturating_add(1);
                    } else {
                        state.panic_signal = Some(signal);
                    }
                }
            }
            state.ingest_line(&line, ssid_marker, require_listener);
            if state.expected_soft_reset_reboot_seen {
                self.handle_expected_soft_reset_reboot(&mut state)?;
            }
            if state.panic_signal.is_some() {
                break;
            }
        }
        Ok(state)
    }

    fn handle_expected_soft_reset_reboot(&mut self, state: &mut ProbeRoundState) -> Result<()> {
        state.expected_soft_reset_reboot_seen = false;
        self.logger.info(format!(
            "expected_soft_reset_reboot_detected: reapplying NETCFG/NET START (recovery={}/{})",
            state.expected_soft_reset_recoveries, MAX_EXPECTED_SOFT_RESET_RECOVERIES_PER_ROUND
        ));
        thread::sleep(Duration::from_millis(1200));
        let payload = netcfg_set_payload(&self.ssid, &self.password, self.policy);
        wait_net_ack(&mut self.console, &format!("NETCFG SET {payload}"))?;
        if self.profile.disable_listener_during_probe_rounds {
            let _ = wait_net_ack(&mut self.console, "NET LISTENER OFF");
        }
        let _ = wait_net_ack(&mut self.console, "NET START");
        Ok(())
    }

    fn send_status_poll_if_ready(&mut self, state: &mut ProbeRoundState) {
        if Instant::now() >= state.next_status_poll {
            let _ = self.console.send_line("NET STATUS");
            state.next_status_poll =
                Instant::now() + Duration::from_millis(self.profile.status_poll_ms as u64);
        }
    }

    fn maybe_recover_after_round(&mut self, round: u32) -> Result<()> {
        self.run_control_command_with_guard(
            round,
            "NET RECOVER",
            self.profile.recover_settle_ms as u64,
        )
    }
}
