use std::net::TcpStream;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

use super::line_checks::{
    headroom_minimum, metrics_listener, snooped_listener, stack_status_values,
};
use super::{AssetResidencyRuntime, UPLOAD_HTTP_PORT};

impl AssetResidencyRuntime<'_> {
    pub(super) fn dispatch_telemetry_actions(
        &mut self,
        action: &str,
        args: &Value,
    ) -> Option<Result<()>> {
        match action {
            "metrics_sample" => Some(self.metrics_sample(args)),
            "net_liveness" => Some(self.net_liveness(args)),
            _ => None,
        }
    }

    fn metrics_sample(&mut self, args: &Value) -> Result<()> {
        // Keep METRICS for network telemetry, then query the correlated
        // STACKSTATUS response for stack minima and diagnostic loss.
        // Either reply proves serial liveness; only a miss of both
        // increments the silence streak.
        let settle_secs = args.get("settle_secs").and_then(Value::as_u64).unwrap_or(0);
        let abort_after_consecutive_misses = args
            .get("abort_after_consecutive_misses")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if settle_secs > 0 {
            std::thread::sleep(Duration::from_secs(settle_secs));
        }
        let mark = self.console.mark();
        let mut trend = format!("metrics_sample elapsed_ms={}", self.elapsed_ms());
        let mut main_minimum = "missing".to_string();
        let mut touch_minimum = "missing".to_string();
        let mut tx_drop = "missing".to_string();
        let mut metrics_answered = false;
        if self.console.send_line("METRICS").is_ok() {
            let terminal = Regex::new(r"^METRICS (?:NET_ACCEPT |BUSY$|ERR )")?;
            if self
                .console
                .wait_for_regex_since(mark, &terminal, Duration::from_secs(15))?
                .is_some()
            {
                metrics_answered = true;
                let stack_re = Regex::new(r"^stack_diag: tag=minimum headroom=(\d+)")?;
                let touch_re = Regex::new(r"^touch_core_stack_diag: tag=minimum headroom=(\d+)")?;
                if let Some(line) = self.console.find_first_regex_since(mark, &stack_re) {
                    if let Some(parsed) = headroom_minimum(&line) {
                        main_minimum = parsed;
                    }
                }
                if let Some(line) = self.console.find_first_regex_since(mark, &touch_re) {
                    if let Some(parsed) = headroom_minimum(&line) {
                        touch_minimum = parsed;
                    }
                }
            }
        }
        let stack_re = Regex::new(r"^STACK_STATUS cpu0=[0-9]+ touch=[0-9]+ tx_drop=[0-9]+$")?;
        let stack_line =
            self.console
                .command_wait_regex("STACKSTATUS", &stack_re, Duration::from_secs(5))?;
        let stack_answered = stack_line
            .as_deref()
            .and_then(stack_status_values)
            .map(|(cpu0, touch, drops)| {
                main_minimum = cpu0.to_string();
                touch_minimum = touch.to_string();
                tx_drop = drops.to_string();
            })
            .is_some();
        trend.push_str(&format!(
            " main_minimum={main_minimum} touch_minimum={touch_minimum} tx_drop={tx_drop} metrics_answered={} stack_answered={}",
            u8::from(metrics_answered),
            u8::from(stack_answered),
        ));
        self.state_lines.push(trend);
        if metrics_answered || stack_answered {
            self.consecutive_misses = 0;
        } else {
            self.consecutive_misses = self.consecutive_misses.saturating_add(1);
        }
        if abort_after_consecutive_misses > 0
            && u64::from(self.consecutive_misses) >= abort_after_consecutive_misses
        {
            return Err(anyhow!(
                "device silent: {} consecutive METRICS/STACKSTATUS samples unanswered (last stream activity {:.0}s ago, net {}); short-circuiting to the report",
                self.consecutive_misses,
                self.console.silence_duration().as_secs_f32(),
                self.last_net.as_deref().unwrap_or("unknown (no probe yet)")
            ));
        }
        Ok(())
    }

    fn net_liveness(&mut self, args: &Value) -> Result<()> {
        // Wi-Fi side-channel for the hang hunt: TCP-handshake the
        // firmware's upload listener and record one trend line per
        // grid point. Prefer the freshest listener announcement or
        // live METRICS NET status, so a lost one-time announcement
        // does not silently void the TCP half of the test. Serial
        // silence + net up means the UART path died
        // with the CPU alive; silence + net down means a full
        // stall. Sends no bytes: connect, then drop.
        let timeout_secs = args
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(5);
        let listener_port = args
            .get("port")
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|port| u16::try_from(port).ok())
                    .ok_or_else(|| anyhow!("net_liveness port must be a valid u16"))
            })
            .transpose()?
            .unwrap_or(UPLOAD_HTTP_PORT);
        let allow_closed = args
            .get("allow_closed_listener")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        // Ingest anything in flight so the snoop below sees the
        // listener line even when no earlier wait pumped the port.
        self.console.settle(250)?;
        let listener_re = Regex::new(r"listening on \S+|^METRICS NET ")?;
        let target = self
            .console
            .last_regex_since(self.run_mark, &listener_re)
            .as_deref()
            .and_then(|line| {
                snooped_listener(line)
                    .or_else(|| metrics_listener(line, listener_port, allow_closed))
            });
        let mut line = format!("net_liveness elapsed_ms={}", self.elapsed_ms());
        let mut responded = false;
        let summary = match target {
            None => {
                line.push_str(" reachable=unknown reason=no_listener_seen");
                "unknown (no listener seen)".to_string()
            }
            Some(addr) => {
                let start = Instant::now();
                match TcpStream::connect_timeout(&addr, Duration::from_secs(timeout_secs)) {
                    Ok(stream) => {
                        drop(stream);
                        responded = true;
                        let rtt_ms = start.elapsed().as_millis();
                        line.push_str(&format!(" reachable=true target={addr} rtt_ms={rtt_ms}"));
                        format!("up {addr} rtt_ms={rtt_ms}")
                    }
                    Err(err) => {
                        responded = err.kind() == std::io::ErrorKind::ConnectionRefused;
                        line.push_str(&format!(
                            " reachable=false target={addr} err={:?}",
                            err.kind()
                        ));
                        format!("down {addr} err={:?}", err.kind())
                    }
                }
            }
        };
        self.last_net = Some(summary.clone());
        self.logger.info(format!("net_liveness {summary}"));
        let reachable = line.contains(" reachable=true ");
        line.push_str(&format!(" responded={}", u8::from(responded)));
        self.state_lines.push(line);
        if args
            .get("require_response")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && !responded
        {
            return Err(anyhow!(
                "network probe did not get a TCP response: {summary}"
            ));
        }
        if args
            .get("require_reachable")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && !reachable
        {
            return Err(anyhow!("upload listener not reachable: {summary}"));
        }
        Ok(())
    }
}
