impl WifiDiscoveryRuntime<'_> {
    fn record_round_results(&mut self, round: u32, state: &ProbeRoundState, zero_discovery: bool) {
        let mut scan_zero_events = state.scan_zero_events;
        let mut scan_nonzero_events = state.scan_nonzero_events;
        let mut no_ap_found_events = state.no_ap_found_events;
        let mut ssid_seen_events = state.ssid_seen_events;
        let mut scan_runs_delta = 0u32;

        match self.query_wifi_scan_counters() {
            Ok(Some(counters)) => {
                if let Some(previous) = self.last_wifi_metrics_scan_counters {
                    scan_runs_delta = counters.scan_runs.saturating_sub(previous.scan_runs);
                    let scan_empty_delta = counters.scan_empty.saturating_sub(previous.scan_empty);
                    let scan_hits_delta = counters.scan_hits.saturating_sub(previous.scan_hits);
                    let no_ap_found_delta =
                        counters.no_ap_found.saturating_sub(previous.no_ap_found);
                    scan_zero_events = scan_zero_events.saturating_add(scan_empty_delta);
                    scan_nonzero_events = scan_nonzero_events.saturating_add(scan_hits_delta);
                    no_ap_found_events = no_ap_found_events.saturating_add(no_ap_found_delta);
                    ssid_seen_events = ssid_seen_events.saturating_add(scan_hits_delta);
                }
                self.last_wifi_metrics_scan_counters = Some(counters);
            }
            Ok(None) => {
                self.logger
                    .info(format!("round {round}: METRICS WIFI unavailable"));
            }
            Err(err) => {
                self.logger
                    .info(format!("round {round}: METRICS WIFI query failed ({err})"));
            }
        }

        if state.ready {
            self.ready_rounds = self.ready_rounds.saturating_add(1);
        }
        if zero_discovery {
            self.zero_discovery_rounds = self.zero_discovery_rounds.saturating_add(1);
        }
        if ssid_seen_events > 0 {
            self.ssid_seen_rounds = self.ssid_seen_rounds.saturating_add(1);
        }
        self.total_scan_zero_events = self.total_scan_zero_events.saturating_add(scan_zero_events);
        self.total_scan_nonzero_events = self
            .total_scan_nonzero_events
            .saturating_add(scan_nonzero_events);
        self.total_scan_runs_delta = self.total_scan_runs_delta.saturating_add(scan_runs_delta);
        self.total_no_ap_found_events = self
            .total_no_ap_found_events
            .saturating_add(no_ap_found_events);

        self.samples.push(RoundSample {
            round,
            ready: state.ready,
            zero_discovery,
            scan_zero_events,
            scan_nonzero_events,
            scan_runs_delta,
            no_ap_found_events,
            ssid_seen_events,
            failure_class: state.last_failure_class.clone(),
        });
        self.logger.info(format!(
            "round {}: ready={} zero_discovery={} scan_zero={} scan_nonzero={} scan_runs_delta={} no_ap_found={} ssid_seen={} failure_class={} min_internal_free={} min_total_free={} nomem_stage_samples={}",
            round,
            state.ready,
            zero_discovery,
            scan_zero_events,
            scan_nonzero_events,
            scan_runs_delta,
            no_ap_found_events,
            ssid_seen_events,
            state.last_failure_class,
            fmt_min(&state.round_mem_diag.min_internal_free),
            fmt_min(&state.round_mem_diag.min_free),
            state.round_mem_diag.nomem_stage_samples
        ));
    }

    fn handle_evaluate_results(&mut self, context: &mut Value) -> Result<()> {
        let pass_zero = self.zero_discovery_rounds <= self.profile.max_zero_discovery_rounds;
        let pass_ready = self.ready_rounds >= self.profile.min_ready_rounds;
        let scan_evidence_rounds = self
            .samples
            .iter()
            .filter(|sample| {
                sample.scan_zero_events > 0
                    || sample.scan_nonzero_events > 0
                    || sample.scan_runs_delta > 0
            })
            .count() as u32;
        let pass_scan_evidence = !self.profile.require_scan_evidence_each_round
            || scan_evidence_rounds >= self.samples.len() as u32;
        let observed_scan_activity = self.total_scan_zero_events > 0
            || self.total_scan_nonzero_events > 0
            || self.total_scan_runs_delta > 0;
        let require_ssid_evidence = observed_scan_activity || self.ready_rounds == 0;
        let pass_ssid =
            !require_ssid_evidence || self.ssid_seen_rounds >= self.profile.min_ssid_seen_rounds;
        let passed = pass_zero && pass_ready && pass_scan_evidence && pass_ssid;

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
        if !pass_scan_evidence {
            failures.push(format!(
                "scan_evidence_rounds={} below required_rounds={} (require_scan_evidence_each_round=true)",
                scan_evidence_rounds,
                self.samples.len()
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
        let scan_evidence_rounds = self
            .samples
            .iter()
            .filter(|sample| {
                sample.scan_zero_events > 0
                    || sample.scan_nonzero_events > 0
                    || sample.scan_runs_delta > 0
            })
            .count() as u32;
        self.logger.info(format!(
            "summary rounds={} ready_rounds={} zero_discovery_rounds={} ssid_seen_rounds={} scan_evidence_rounds={} require_scan_evidence_each_round={} total_scan_runs_delta={} total_scan_zero_events={} total_scan_nonzero_events={} total_no_ap_found_events={}",
            self.samples.len(),
            self.ready_rounds,
            self.zero_discovery_rounds,
            self.ssid_seen_rounds,
            scan_evidence_rounds,
            self.profile.require_scan_evidence_each_round,
            self.total_scan_runs_delta,
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
                "round {} detail ready={} zero_discovery={} scan_zero={} scan_nonzero={} scan_runs_delta={} no_ap_found={} ssid_seen={} failure_class={}",
                sample.round,
                sample.ready,
                sample.zero_discovery,
                sample.scan_zero_events,
                sample.scan_nonzero_events,
                sample.scan_runs_delta,
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
