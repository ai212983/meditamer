use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::{
    scenarios::WorkflowRuntime,
    workflows_wifi_common::{
        ctx_get_string, ctx_get_u32, ctx_set_bool, ctx_set_string, ctx_set_u32,
        detect_panic_signal, extract_context_window, fmt_min, netcfg_set_payload, wait_net_ack,
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
        self.panic_first = None;
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
            ctx_set_string(context, "run_error", &detail)?;
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
            let line_index = state.read_mark;
            state.read_mark = state.read_mark.saturating_add(1);
            self.mem_diag.record_line(&line);
            state.round_mem_diag.record_line(&line);
            if state.panic_signal.is_none() {
                state.panic_signal = detect_panic_signal(&line, line_index);
            }
            state.ingest_line(&line, ssid_marker, require_listener);
            if state.panic_signal.is_some() {
                break;
            }
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

    fn panic_signal_detail(&self, signal: &crate::workflows_wifi_common::PanicSignal) -> String {
        let excerpt_start = signal.marker_index.saturating_sub(3);
        let excerpt_source = self.console.read_recent_lines(excerpt_start);
        let excerpt = format_context_excerpt(extract_context_window(
            &excerpt_source,
            excerpt_start,
            signal.marker_index,
            3,
        ));
        match excerpt {
            Some(window) => format!(
                "panic_detected class={} line_index={} line={} context={window}",
                signal.class.as_str(),
                signal.marker_index,
                signal.marker_line
            ),
            None => format!(
                "panic_detected class={} line_index={} line={}",
                signal.class.as_str(),
                signal.marker_index,
                signal.marker_line
            ),
        }
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
        if let Some(signal) = &self.panic_first {
            self.logger.info(format!(
                "summary panic class={} line_index={} line={}",
                signal.class.as_str(),
                signal.marker_index,
                signal.marker_line
            ));
        }
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

fn format_context_excerpt(window: Vec<(usize, String)>) -> Option<String> {
    if window.is_empty() {
        return None;
    }
    Some(
        window
            .into_iter()
            .map(|(idx, line)| format!("{idx}:{line}"))
            .collect::<Vec<_>>()
            .join(" | "),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };

    use anyhow::{anyhow, Result};
    use serde_json::json;
    use serialport::{SerialPort, TTYPort};
    use tempfile::tempdir;

    use super::super::{profile::DiscoveryProfile, WifiDiscoveryRuntime};
    use crate::{
        logging::Logger,
        serial_console::SerialConsole,
        workflows_wifi_common::{MemDiagSummary, NetPolicy},
    };

    fn open_pty_pair() -> Result<(TTYPort, TTYPort)> {
        TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))
    }

    fn spawn_discovery_panic_responder(mut master: TTYPort) -> thread::JoinHandle<()> {
        let _ = master.set_timeout(Duration::from_millis(80));
        thread::spawn(move || {
            let mut rx = Vec::<u8>::new();
            let mut chunk = [0u8; 512];
            let mut emitted_panic = false;
            let mut last_activity = Instant::now();

            loop {
                let n = match master.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        last_activity = Instant::now();
                        n
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                        if last_activity.elapsed() > Duration::from_secs(2) {
                            break;
                        }
                        continue;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        if last_activity.elapsed() > Duration::from_secs(2) {
                            break;
                        }
                        continue;
                    }
                    Err(_) => break,
                };
                rx.extend_from_slice(&chunk[..n]);

                while let Some(pos) = rx.iter().position(|byte| *byte == b'\n') {
                    let mut line = rx.drain(..=pos).collect::<Vec<u8>>();
                    while matches!(line.last(), Some(b'\r' | b'\n')) {
                        line.pop();
                    }
                    if line.is_empty() {
                        continue;
                    }
                    let command = String::from_utf8_lossy(&line).trim().to_string();
                    if command == "NET START" {
                        let _ = master.write_all(b"NET OK op=start\r\n");
                        let _ = master.flush();
                    } else if command == "NET STATUS" {
                        let _ = master.write_all(
                            b"NET_STATUS {\"state\":\"Idle\",\"link\":false,\"ipv4\":\"0.0.0.0\",\"listener\":false,\"listener_enabled\":false}\r\n",
                        );
                        if !emitted_panic {
                            let _ = master.write_all(
                                b"Guru Meditation Error: Core 0 panic'ed (LoadProhibited)\r\n",
                            );
                            emitted_panic = true;
                        }
                        let _ = master.flush();
                    }
                }
            }
        })
    }

    fn test_console(slave: TTYPort) -> Result<SerialConsole> {
        let temp = tempdir()?;
        let log_path = PathBuf::from(temp.path()).join("discovery_runtime_panic_test.log");
        SerialConsole::from_port_for_tests(Box::new(slave), Some(&log_path))
    }

    #[test]
    fn probe_round_fails_fast_when_panic_marker_is_seen() -> Result<()> {
        let (master, slave) = open_pty_pair()?;
        let responder = spawn_discovery_panic_responder(master);
        let mut logger = Logger::new(None)?;
        let mut runtime = WifiDiscoveryRuntime {
            logger: &mut logger,
            console: test_console(slave)?,
            ssid: "test-ap".to_string(),
            password: "test-pass".to_string(),
            policy: NetPolicy::default(),
            profile: DiscoveryProfile {
                rounds: 1,
                round_timeout_ms: 2_000,
                poll_interval_ms: 30,
                status_poll_ms: 120,
                recover_before_round: false,
                recover_after_round: false,
                recover_settle_ms: 6_000,
                disable_listener_during_probe_rounds: false,
                max_zero_discovery_rounds: 0,
                min_ready_rounds: 1,
                min_ssid_seen_rounds: 1,
            },
            samples: Vec::new(),
            ready_rounds: 0,
            zero_discovery_rounds: 0,
            ssid_seen_rounds: 0,
            total_scan_zero_events: 0,
            total_scan_nonzero_events: 0,
            total_no_ap_found_events: 0,
            mem_diag: MemDiagSummary::default(),
            panic_first: None,
        };

        let mut context = json!({"round": 1});
        let err = runtime
            .handle_probe_round(&mut context)
            .expect_err("panic marker should fail round immediately");
        assert!(err.to_string().contains("panic_detected"));
        assert!(err.to_string().contains("runtime_panic_guru"));
        assert!(runtime.panic_first.is_some());

        drop(runtime);
        responder
            .join()
            .map_err(|_| anyhow!("discovery panic responder thread panicked"))?;
        Ok(())
    }
}
