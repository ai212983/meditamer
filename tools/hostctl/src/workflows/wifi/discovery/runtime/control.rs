impl WifiDiscoveryRuntime<'_> {
    fn handle_start_run(&mut self, context: &mut Value) -> Result<()> {
        ctx_set_u32(context, "rounds", self.profile.rounds)?;
        ctx_set_bool(context, "run_passed", false)?;
        ctx_set_string(context, "run_error", "")?;
        self.panic_first = None;
        if self.profile.disable_listener_during_probe_rounds {
            self.run_control_command_with_guard(0, "NET LISTENER OFF", 0)?;
        }
        self.logger.info(format!(
            "wifi-discovery-debug: effective_round_timeout_ms={} (profile_round_timeout_ms={})",
            recommended_round_timeout_ms(&self.policy, &self.profile),
            self.profile.round_timeout_ms
        ));
        self.last_wifi_metrics_scan_counters = None;
        match self.query_wifi_scan_counters() {
            Ok(Some(counters)) => {
                self.last_wifi_metrics_scan_counters = Some(counters);
                self.logger.info(format!(
                    "wifi-discovery-debug: metrics baseline no_ap={} scan_runs={} scan_empty={} scan_hits={}",
                    counters.no_ap_found,
                    counters.scan_runs,
                    counters.scan_empty,
                    counters.scan_hits
                ));
            }
            Ok(None) => {
                self.logger
                    .info("wifi-discovery-debug: METRICS WIFI baseline unavailable (missing line)");
            }
            Err(err) => {
                self.logger.info(format!(
                    "wifi-discovery-debug: METRICS WIFI baseline query failed ({err})"
                ));
            }
        }
        Ok(())
    }

    fn handle_net_apply_config(&mut self) -> Result<()> {
        let payload = netcfg_set_payload(&self.ssid, &self.password, self.policy);
        wait_net_ack(&mut self.console, &format!("NETCFG SET {payload}"))
    }

    fn run_control_command_with_guard(
        &mut self,
        round: u32,
        command: &str,
        settle_ms: u64,
    ) -> Result<()> {
        let first_err = match wait_net_ack(&mut self.console, command) {
            Ok(()) => {
                if settle_ms > 0 {
                    self.console.settle(settle_ms)?;
                }
                return Ok(());
            }
            Err(err) => err,
        };
        let command_token = control_command_token(command);
        self.logger.info(format!(
            "round {round}: control_ack_loss command={command_token} err={first_err}; attempting recovery"
        ));

        self.recover_control_channel_after_ack_loss(round, command_token, command, &first_err)?;
        wait_net_ack(&mut self.console, command).map_err(|retry_err| {
            let detail = format!(
                "round {round}: control ack lost command={command_token} first_err={first_err} retry_err={retry_err}"
            );
            self.logger.info(format!(
                "HOST_FAILURE class=host_transport_control_ack_loss command={command_token} round={round} stage=retry detail={retry_err}"
            ));
            anyhow!("{detail}")
        })?;

        self.logger.info(format!(
            "round {round}: control_ack_recovered command={command_token}"
        ));
        if settle_ms > 0 {
            self.console.settle(settle_ms)?;
        }
        Ok(())
    }

    fn recover_control_channel_after_ack_loss(
        &mut self,
        round: u32,
        command_token: &str,
        command: &str,
        first_err: &anyhow::Error,
    ) -> Result<()> {
        self.console.settle(ACK_LOSS_RECOVERY_SETTLE_MS)?;
        preflight(&mut self.console).map_err(|preflight_err| {
            self.logger.info(format!(
                "HOST_FAILURE class=host_transport_control_ack_loss command={command_token} round={round} stage=preflight detail={preflight_err}"
            ));
            anyhow!(
                "round {round}: control ack lost command={command_token} first_err={first_err} preflight_err={preflight_err}"
            )
        })?;

        if command.trim_start().starts_with("NET RECOVER") {
            return Ok(());
        }

        wait_net_ack(&mut self.console, "NET RECOVER").map_err(|recover_err| {
            self.logger.info(format!(
                "HOST_FAILURE class=host_transport_control_ack_loss command={command_token} round={round} stage=recover detail={recover_err}"
            ));
            anyhow!(
                "round {round}: control ack lost command={command_token} first_err={first_err} recover_err={recover_err}"
            )
        })?;
        self.console.settle(self.profile.recover_settle_ms as u64)?;
        Ok(())
    }

    fn query_wifi_scan_counters(&mut self) -> Result<Option<WifiMetricsScanCounters>> {
        let metrics_wifi_re = Regex::new(r"^METRICS WIFI ")?;
        let line =
            self.console
                .command_wait_regex("METRICS", &metrics_wifi_re, Duration::from_secs(3))?;
        Ok(line.and_then(|value| parse_wifi_scan_counters_line(&value)))
    }

    fn panic_signal_detail(&self, signal: &crate::workflows::wifi::common::PanicSignal) -> String {
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
}

fn parse_wifi_scan_counters_line(line: &str) -> Option<WifiMetricsScanCounters> {
    if !line.starts_with("METRICS WIFI ") {
        return None;
    }
    Some(WifiMetricsScanCounters {
        scan_runs: parse_metrics_key_u32(line, "scan_runs")?,
        scan_empty: parse_metrics_key_u32(line, "scan_empty")?,
        scan_hits: parse_metrics_key_u32(line, "scan_hits")?,
        no_ap_found: parse_metrics_key_u32(line, "no_ap")?,
    })
}

fn parse_metrics_key_u32(line: &str, key: &str) -> Option<u32> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
        .and_then(|value| value.parse::<u32>().ok())
}

fn control_command_token(command: &str) -> &'static str {
    let trimmed = command.trim_start();
    if trimmed.starts_with("NET STOP") {
        "net_stop"
    } else if trimmed.starts_with("NET RECOVER") {
        "net_recover"
    } else if trimmed.starts_with("NET LISTENER OFF") {
        "net_listener_off"
    } else if trimmed.starts_with("NET LISTENER ON") {
        "net_listener_on"
    } else if trimmed.starts_with("NET START") {
        "net_start"
    } else if trimmed.starts_with("NETCFG SET") {
        "netcfg_set"
    } else {
        "net_control"
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
