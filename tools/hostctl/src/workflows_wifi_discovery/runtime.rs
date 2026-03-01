use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::{
    scenarios::WorkflowRuntime,
    workflows_wifi_common::{
        ctx_get_string, ctx_get_u32, ctx_set_bool, ctx_set_string, ctx_set_u32, fmt_min,
        netcfg_set_payload, wait_net_ack,
    },
};

use super::{
    probe::{ProbeRoundState, RoundSample},
    profile::recommended_round_timeout_ms,
    WifiDiscoveryRuntime,
};

impl WorkflowRuntime for WifiDiscoveryRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "start_run" => self.handle_start_run(context),
            "net_apply_config" => self.handle_net_apply_config(),
            "probe_round" => self.handle_probe_round(context),
            "advance_round" => self.handle_advance_round(context),
            "evaluate_results" => self.handle_evaluate_results(context),
            "print_summary" => self.handle_print_summary(),
            "fail_run" => self.handle_fail_run(context),
            _ => Err(anyhow!("unknown workflow action: {action}")),
        }
    }
}

impl WifiDiscoveryRuntime<'_> {
    fn handle_start_run(&mut self, context: &mut Value) -> Result<()> {
        ctx_set_u32(context, "round", 1)?;
        ctx_set_u32(context, "rounds", self.profile.rounds)?;
        ctx_set_bool(context, "run_passed", false)?;
        ctx_set_string(context, "run_error", "")?;
        if self.profile.disable_listener_during_probe_rounds {
            wait_net_ack(&mut self.console, "NET LISTENER OFF")?;
        }
        self.logger.info(format!(
            "wifi-discovery-debug: effective_round_timeout_ms={} (profile_round_timeout_ms={})",
            recommended_round_timeout_ms(&self.policy, &self.profile),
            self.profile.round_timeout_ms
        ));
        Ok(())
    }

    fn handle_net_apply_config(&mut self) -> Result<()> {
        let payload = netcfg_set_payload(&self.ssid, &self.password, self.policy);
        wait_net_ack(&mut self.console, &format!("NETCFG SET {payload}"))
    }

    fn handle_probe_round(&mut self, context: &mut Value) -> Result<()> {
        let round = ctx_get_u32(context, "round")?;
        let ssid_marker = format!("scan ap ssid={}", self.ssid);
        let require_listener = !self.profile.disable_listener_during_probe_rounds;

        if let Err(err) = self.maybe_recover_before_round(round) {
            self.logger.info(format!(
                "round {round}: NET RECOVER before probe failed ({err})"
            ));
        }
        self.prepare_probe_round(round);
        if let Err(err) = wait_net_ack(&mut self.console, "NET START") {
            self.logger
                .info(format!("round {round}: NET START ack not obtained ({err})"));
        }

        let mut state = self.new_probe_round_state();
        while Instant::now() < state.deadline {
            state = self.collect_probe_round_lines(state, &ssid_marker, require_listener)?;
            if state.ready {
                break;
            }
            self.send_status_poll_if_ready(&mut state);
            thread::sleep(Duration::from_millis(self.profile.poll_interval_ms as u64));
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
        ctx_set_bool(context, "round_ready", state.ready)?;
        ctx_set_bool(context, "round_zero_discovery", zero_discovery)?;
        Ok(())
    }

    fn maybe_recover_before_round(&mut self, _round: u32) -> Result<()> {
        if !self.profile.recover_before_round {
            return Ok(());
        }
        wait_net_ack(&mut self.console, "NET RECOVER")?;
        self.console.settle(self.profile.recover_settle_ms as u64)?;
        Ok(())
    }

    fn prepare_probe_round(&mut self, round: u32) {
        if !self.profile.disable_listener_during_probe_rounds {
            return;
        }
        if let Err(err) = wait_net_ack(&mut self.console, "NET LISTENER OFF") {
            self.logger
                .info(format!("round {round}: NET LISTENER OFF failed ({err})"));
        }
    }

    fn new_probe_round_state(&self) -> ProbeRoundState {
        ProbeRoundState::new(
            self.console.mark(),
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
            state.read_mark = state.read_mark.saturating_add(1);
            self.mem_diag.record_line(&line);
            state.round_mem_diag.record_line(&line);
            state.ingest_line(&line, ssid_marker, require_listener);
        }
        Ok(state)
    }

    fn send_status_poll_if_ready(&mut self, state: &mut ProbeRoundState) {
        if Instant::now() >= state.next_status_poll {
            let _ = self.console.send_line("NET STATUS");
            state.next_status_poll =
                Instant::now() + Duration::from_millis(self.profile.status_poll_ms as u64);
        }
    }

    fn maybe_recover_after_round(&mut self, round: u32) -> Result<()> {
        wait_net_ack(&mut self.console, "NET RECOVER")
            .map_err(|err| anyhow!("round {round}: NET RECOVER after probe failed ({err})"))?;
        self.console.settle(self.profile.recover_settle_ms as u64)?;
        Ok(())
    }

    fn record_round_results(&mut self, round: u32, state: &ProbeRoundState, zero_discovery: bool) {
        if state.ready {
            self.ready_rounds = self.ready_rounds.saturating_add(1);
        }
        if zero_discovery {
            self.zero_discovery_rounds = self.zero_discovery_rounds.saturating_add(1);
        }
        if state.ssid_seen_events > 0 {
            self.ssid_seen_rounds = self.ssid_seen_rounds.saturating_add(1);
        }
        self.total_scan_zero_events = self
            .total_scan_zero_events
            .saturating_add(state.scan_zero_events);
        self.total_scan_nonzero_events = self
            .total_scan_nonzero_events
            .saturating_add(state.scan_nonzero_events);
        self.total_no_ap_found_events = self
            .total_no_ap_found_events
            .saturating_add(state.no_ap_found_events);

        self.samples.push(RoundSample {
            round,
            ready: state.ready,
            zero_discovery,
            scan_zero_events: state.scan_zero_events,
            scan_nonzero_events: state.scan_nonzero_events,
            no_ap_found_events: state.no_ap_found_events,
            ssid_seen_events: state.ssid_seen_events,
            failure_class: state.last_failure_class.clone(),
        });
        self.logger.info(format!(
            "round {}: ready={} zero_discovery={} scan_zero={} scan_nonzero={} no_ap_found={} ssid_seen={} failure_class={} min_internal_free={} min_total_free={} nomem_stage_samples={}",
            round,
            state.ready,
            zero_discovery,
            state.scan_zero_events,
            state.scan_nonzero_events,
            state.no_ap_found_events,
            state.ssid_seen_events,
            state.last_failure_class,
            fmt_min(&state.round_mem_diag.min_internal_free),
            fmt_min(&state.round_mem_diag.min_free),
            state.round_mem_diag.nomem_stage_samples
        ));
    }

    fn handle_advance_round(&mut self, context: &mut Value) -> Result<()> {
        let round = ctx_get_u32(context, "round")?;
        ctx_set_u32(context, "round", round.saturating_add(1))
    }

    fn handle_evaluate_results(&mut self, context: &mut Value) -> Result<()> {
        let pass_zero = self.zero_discovery_rounds <= self.profile.max_zero_discovery_rounds;
        let pass_ready = self.ready_rounds >= self.profile.min_ready_rounds;
        let observed_scan_activity =
            self.total_scan_zero_events > 0 || self.total_scan_nonzero_events > 0;
        let require_ssid_evidence = observed_scan_activity || self.ready_rounds == 0;
        let pass_ssid =
            !require_ssid_evidence || self.ssid_seen_rounds >= self.profile.min_ssid_seen_rounds;
        let passed = pass_zero && pass_ready && pass_ssid;

        let mut failures = Vec::new();
        if !pass_zero {
            failures.push(format!(
                "zero_discovery_rounds={} exceeds max_zero_discovery_rounds={}",
                self.zero_discovery_rounds, self.profile.max_zero_discovery_rounds
            ));
        }
        if !pass_ready {
            failures.push(format!(
                "ready_rounds={} below min_ready_rounds={}",
                self.ready_rounds, self.profile.min_ready_rounds
            ));
        }
        if !pass_ssid {
            failures.push(format!(
                "ssid_seen_rounds={} below min_ssid_seen_rounds={}",
                self.ssid_seen_rounds, self.profile.min_ssid_seen_rounds
            ));
        }
        let run_error = failures.join("; ");
        ctx_set_bool(context, "run_passed", passed)?;
        ctx_set_string(context, "run_error", &run_error)?;
        Ok(())
    }

    fn handle_print_summary(&mut self) -> Result<()> {
        self.logger.info(format!(
            "summary rounds={} ready_rounds={} zero_discovery_rounds={} ssid_seen_rounds={} total_scan_zero_events={} total_scan_nonzero_events={} total_no_ap_found_events={}",
            self.samples.len(),
            self.ready_rounds,
            self.zero_discovery_rounds,
            self.ssid_seen_rounds,
            self.total_scan_zero_events,
            self.total_scan_nonzero_events,
            self.total_no_ap_found_events
        ));
        self.logger.info(format!(
            "summary mem samples={} radio_samples={} upload_samples={} nomem_stage_samples={} min_internal_free={} min_external_free={} min_total_free={} min_internal_low_water={}",
            self.mem_diag.samples,
            self.mem_diag.radio_samples,
            self.mem_diag.upload_samples,
            self.mem_diag.nomem_stage_samples,
            fmt_min(&self.mem_diag.min_internal_free),
            fmt_min(&self.mem_diag.min_external_free),
            fmt_min(&self.mem_diag.min_free),
            fmt_min(&self.mem_diag.min_internal_low_water),
        ));
        for sample in &self.samples {
            self.logger.info(format!(
                "round {} detail ready={} zero_discovery={} scan_zero={} scan_nonzero={} no_ap_found={} ssid_seen={} failure_class={}",
                sample.round,
                sample.ready,
                sample.zero_discovery,
                sample.scan_zero_events,
                sample.scan_nonzero_events,
                sample.no_ap_found_events,
                sample.ssid_seen_events,
                sample.failure_class
            ));
        }
        if self.profile.disable_listener_during_probe_rounds {
            if let Err(err) = wait_net_ack(&mut self.console, "NET LISTENER ON") {
                self.logger
                    .info(format!("summary: NET LISTENER ON restore failed ({err})"));
            }
        }
        Ok(())
    }

    fn handle_fail_run(&mut self, context: &mut Value) -> Result<()> {
        if self.profile.disable_listener_during_probe_rounds {
            if let Err(err) = wait_net_ack(&mut self.console, "NET LISTENER ON") {
                self.logger
                    .info(format!("fail_run: NET LISTENER ON restore failed ({err})"));
            }
        }
        let detail = ctx_get_string(context, "run_error")
            .unwrap_or_else(|_| "wifi discovery debug failed".to_string());
        Err(anyhow!("{detail}"))
    }
}
