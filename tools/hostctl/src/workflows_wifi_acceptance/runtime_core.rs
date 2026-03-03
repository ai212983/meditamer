use std::{fs, thread, time::Duration};

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::{
    logging::ensure_parent_dir,
    scenarios::WorkflowRuntime,
    workflows_wifi_common::{
        ctx_set_bool, ctx_set_string, ctx_set_u32, detect_panic_signal, extract_context_window,
        fmt_min, is_ready, netcfg_set_payload, query_net_status, wait_net_ack, NetStatus,
        PanicSignal,
    },
};

use super::{
    wait_ready::{wait_ready, wait_state_progress},
    WifiAcceptanceRuntime,
};

impl WifiAcceptanceRuntime<'_> {
    pub(super) fn capture_mem_diag_lines(&mut self) -> Result<()> {
        self.console.poll_once()?;
        for line in self.console.read_recent_lines(self.mem_read_mark) {
            let line_index = self.mem_read_mark;
            self.mem_read_mark = self.mem_read_mark.saturating_add(1);
            self.mem_diag.record_line(&line);
            if self.panic_monitoring_enabled && self.panic_first.is_none() {
                if let Some(signal) = detect_panic_signal(&line, line_index) {
                    self.panic_first = Some(signal.clone());
                    return Err(anyhow!(self.panic_signal_detail(&signal)));
                }
            }
        }
        Ok(())
    }

    pub(super) fn panic_signal(&self) -> Option<&PanicSignal> {
        self.panic_first.as_ref()
    }

    fn panic_signal_detail(&self, signal: &PanicSignal) -> String {
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

    pub(super) fn log_mem_summary(&mut self, prefix: &str) {
        self.logger.info(format!(
            "{prefix} mem samples={} radio_samples={} upload_samples={} nomem_stage_samples={} min_internal_free={} min_external_free={} min_total_free={} min_internal_low_water={}",
            self.mem_diag.samples,
            self.mem_diag.radio_samples,
            self.mem_diag.upload_samples,
            self.mem_diag.nomem_stage_samples,
            fmt_min(&self.mem_diag.min_internal_free),
            fmt_min(&self.mem_diag.min_external_free),
            fmt_min(&self.mem_diag.min_free),
            fmt_min(&self.mem_diag.min_internal_low_water),
        ));
    }
}

impl WorkflowRuntime for WifiAcceptanceRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        self.capture_mem_diag_lines()?;
        let result = match action {
            "prepare_payload" => self.handle_prepare_payload(),
            "start_run" => self.handle_start_run(context),
            "net_apply_config" => self.handle_net_apply_config(),
            "net_start" => self.handle_net_start(),
            "net_wait_state" => {
                wait_state_progress(&mut self.console, self.policy.connect_timeout_ms)
            }
            "net_wait_ready" => self.handle_net_wait_ready(context),
            "init_upload_attempt" => self.handle_init_upload_attempt(context),
            "net_upload_once" => self.handle_net_upload_once(context),
            "net_verify_once" => self.handle_net_verify_once(context),
            "assert_upload_metrics" => self.handle_assert_upload_metrics(context),
            "net_collect_diag" => self.handle_net_collect_diag(),
            "net_recover_once" => self.handle_net_recover_once(),
            "increment_upload_attempt" => self.handle_increment_upload_attempt(context),
            "fail_upload" | "net_fail" => self.handle_fail_upload(context),
            "finalize_cycle" => self.handle_finalize_cycle(context),
            "advance_cycle" => self.handle_advance_cycle(context),
            "print_summary" => self.handle_print_summary(),
            _ => Err(anyhow!("unknown workflow action: {action}")),
        };
        self.capture_mem_diag_lines()?;
        result
    }
}

impl WifiAcceptanceRuntime<'_> {
    fn handle_prepare_payload(&mut self) -> Result<()> {
        ensure_parent_dir(&self.payload_path)?;
        let mut data = vec![0u8; 524_288];
        for (i, slot) in data.iter_mut().enumerate() {
            *slot = ((i * 17 + 31) & 0xFF) as u8;
        }
        fs::write(&self.payload_path, data)?;
        Ok(())
    }

    fn handle_start_run(&mut self, context: &mut Value) -> Result<()> {
        ctx_set_u32(context, "cycle", 1)?;
        ctx_set_u32(context, "cycles", self.cycles)?;
        ctx_set_u32(context, "operation_retries", self.operation_retries)?;
        self.mem_read_mark = self.console.mark();
        self.panic_monitoring_enabled = true;
        self.panic_first = None;
        self.req_read_body_reset_baseline = Some(self.query_req_read_body_reset()?);
        Ok(())
    }

    fn handle_net_apply_config(&mut self) -> Result<()> {
        if query_net_status(&mut self.console)?
            .as_ref()
            .is_some_and(|status| is_ready(status, true))
        {
            self.logger
                .info("net_apply_config: skip NETCFG SET because network is already ready");
            return Ok(());
        }
        let payload = netcfg_set_payload(&self.ssid, &self.password, self.policy);
        wait_net_ack(&mut self.console, &format!("NETCFG SET {payload}"))
    }

    fn handle_net_start(&mut self) -> Result<()> {
        if self.should_skip_net_start_if_ready()? {
            return Ok(());
        }
        self.ensure_listener_on()?;
        if let Some(status) = query_net_status(&mut self.console)? {
            if self.should_return_from_status(status)? {
                return Ok(());
            }
        }
        self.ensure_net_start_ack()?;
        Ok(())
    }

    fn should_skip_net_start_if_ready(&mut self) -> Result<bool> {
        if query_net_status(&mut self.console)?
            .as_ref()
            .is_some_and(|status| is_ready(status, true))
        {
            self.logger
                .info("net_start: skip NET START because network is already ready");
            return Ok(true);
        }
        Ok(false)
    }

    fn ensure_listener_on(&mut self) -> Result<()> {
        if let Err(err) = wait_net_ack(&mut self.console, "NET LISTENER ON") {
            if !self.is_listener_ready()? {
                return Err(anyhow!("net_start: listener enable failed ({err})"));
            }
            self.logger.info(format!(
                "net_start: listener enable ack not obtained ({err}); continuing because listener is already ready"
            ));
        }
        Ok(())
    }

    fn is_listener_ready(&mut self) -> Result<bool> {
        Ok(query_net_status(&mut self.console)?
            .as_ref()
            .is_some_and(|status| {
                matches!(status.state.as_deref(), Some("Ready"))
                    && status.link.unwrap_or(false)
                    && status.listener.unwrap_or(false)
                    && status.listener_enabled.unwrap_or(true)
                    && status.ipv4.as_deref().is_some_and(|ip| ip != "0.0.0.0")
            }))
    }

    fn should_return_from_status(&mut self, status: NetStatus) -> Result<bool> {
        if is_ready(&status, true) {
            self.logger
                .info("net_start: skip NET START because listener is already ready");
            return Ok(true);
        }
        if matches!(status.state.as_deref(), Some("Ready"))
            && status.link.unwrap_or(false)
            && status.ipv4.as_deref().is_some_and(|ip| ip != "0.0.0.0")
            && !status.listener_enabled.unwrap_or(true)
        {
            self.logger.info(
                "net_start: listener gate is disabled while network is Ready; forcing NET LISTENER ON",
            );
            self.ensure_listener_on()?;
            return Ok(true);
        }
        if self.is_stuck_listener_wait(&status) {
            self.logger.info(format!(
                "net_start: state={} with ipv4=0.0.0.0; forcing NET RECOVER",
                status.state.as_deref().unwrap_or("unknown")
            ));
            wait_net_ack(&mut self.console, "NET RECOVER")?;
            thread::sleep(Duration::from_millis(self.policy.cooldown_ms as u64));
            return Ok(true);
        }
        if self.is_active_state(&status) {
            self.logger.info(format!(
                "net_start: skip NET START because state is already active ({})",
                status.state.as_deref().unwrap_or("unknown")
            ));
            return Ok(true);
        }
        Ok(false)
    }

    fn is_stuck_listener_wait(&self, status: &NetStatus) -> bool {
        let ipv4_zero = status.ipv4.as_deref().is_none_or(|ip| ip == "0.0.0.0");
        matches!(status.state.as_deref(), Some("ListenerWait" | "DhcpWait")) && ipv4_zero
    }

    fn is_active_state(&self, status: &NetStatus) -> bool {
        matches!(
            status.state.as_deref(),
            Some(
                "Ready"
                    | "Recovering"
                    | "Starting"
                    | "Scanning"
                    | "Associating"
                    | "DhcpWait"
                    | "ListenerWait"
            )
        )
    }

    fn ensure_net_start_ack(&mut self) -> Result<()> {
        if let Err(err) = wait_net_ack(&mut self.console, "NET START") {
            if !query_net_status(&mut self.console)?.is_some_and(|status| is_ready(&status, true)) {
                return Err(err);
            }
            self.logger.info(format!(
                "net_start: start ack not obtained ({err}); continuing because network is already ready"
            ));
        }
        Ok(())
    }

    fn handle_net_wait_ready(&mut self, context: &mut Value) -> Result<()> {
        let (connect_ms, listen_ms, ip) = wait_ready(&mut self.console, self.policy)?;
        ctx_set_u32(context, "connect_ms", connect_ms)?;
        ctx_set_u32(context, "listen_ms", listen_ms)?;
        ctx_set_string(context, "ip", &ip)?;
        self.connect_samples.push(connect_ms as f64 / 1000.0);
        self.listen_samples.push(listen_ms as f64 / 1000.0);
        Ok(())
    }

    fn handle_init_upload_attempt(&self, context: &mut Value) -> Result<()> {
        ctx_set_u32(context, "upload_attempt", 1)?;
        ctx_set_bool(context, "upload_done", false)?;
        Ok(())
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
