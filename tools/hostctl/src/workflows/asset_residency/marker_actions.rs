use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

use super::line_checks::{mountain_ui_status_values, verify_resident_timing};
use super::markers::marker_pattern;
use super::AssetResidencyRuntime;
use crate::serial_console::AckStatus;

impl AssetResidencyRuntime<'_> {
    pub(super) fn dispatch_marker_actions(
        &mut self,
        action: &str,
        args: &Value,
    ) -> Option<Result<()>> {
        match action {
            "ui_command" => Some(self.ui_command(args)),
            "await_marker" => Some(self.await_marker(args)),
            "record_mountain_timing" => Some(self.record_mountain_timing()),
            "verify_mountain_ui_status" => Some(self.verify_mountain_ui_status(args)),
            _ => None,
        }
    }

    fn ui_command(&mut self, args: &Value) -> Result<()> {
        let command = match super::arg_str(args, "target") {
            Some(target) => format!("UISTEP {target}"),
            None => "UISTEP".to_string(),
        };
        let mark = self.console.mark();
        self.console.send_line(&command)?;
        let (status, line) = self
            .console
            .wait_ack_since(mark, "UISTEP", self.marker_timeout)?;
        match status {
            AckStatus::Ok => Ok(()),
            AckStatus::None => Err(anyhow!(
                "`{command}` timed out; outcome is ambiguous and the run stopped without retry"
            )),
            AckStatus::Busy | AckStatus::Err => Err(anyhow!(
                "`{command}` failed: {}",
                line.unwrap_or_else(|| "missing response".to_string())
            )),
        }
    }

    fn await_marker(&mut self, args: &Value) -> Result<()> {
        let key =
            super::arg_str(args, "key").ok_or_else(|| anyhow!("await_marker requires key"))?;
        let checkpoint = super::arg_str(args, "checkpoint")
            .ok_or_else(|| anyhow!("await_marker requires checkpoint"))?;
        let from = super::arg_str(args, "from").unwrap_or("phase");
        let pattern = marker_pattern(key)?;
        let implied_by = super::arg_str(args, "implied_by");
        if let Some(implied_by) = implied_by {
            if !matches!(
                (key, implied_by),
                (
                    "mountain_validated" | "mountain_rendered",
                    "mountain_adopted"
                ) | ("mountain_cache_validated", "mountain_released")
                    | (
                        "clock_assets_ready" | "clock_entered",
                        "clock_frame_complete"
                    )
            ) {
                return Err(anyhow!(
                    "unsupported marker implication: {key} implied_by {implied_by}"
                ));
            }
        }
        let fallback = implied_by.map(marker_pattern).transpose()?;
        let search_pattern = if let Some(fallback) = &fallback {
            Regex::new(&format!(
                "(?:{})|(?:{})",
                pattern.as_str(),
                fallback.as_str()
            ))?
        } else {
            pattern.clone()
        };
        let start = self.search_mark(from)?;
        // Optional `silence_limit_secs` fails fast on a hung device:
        // a live device always emits something (boot chatter, tap
        // stream) well inside the limit, so an exceeded silence
        // means the marker can never arrive. Zero disables (legacy
        // full wait). Missing stays non-fatal either way.
        let silence_limit = Duration::from_secs(
            args.get("silence_limit_secs")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
        let line = self.console.wait_for_regex_since_abort_on_silence(
            start,
            &search_pattern,
            self.marker_timeout,
            silence_limit,
        )?;
        match line {
            Some(line) => {
                let status = if pattern.is_match(&line) {
                    "pass"
                } else {
                    "inferred"
                };
                self.record_marker(checkpoint, status, Some(line));
                Ok(())
            }
            None => {
                if !silence_limit.is_zero() && self.console.silence_duration() >= silence_limit {
                    self.logger.info(format!(
                        "await_marker `{key}` aborting early: console silent {:.0}s",
                        self.console.silence_duration().as_secs_f32()
                    ));
                }
                self.record_marker(checkpoint, "missing", None);
                if self.active_cycle.is_some() {
                    Err(anyhow!(
                        "cycle marker `{key}` missing in phase {}",
                        self.phase_name
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }

    fn record_mountain_timing(&mut self) -> Result<()> {
        self.console.settle(100)?;
        let line = self
            .console
            .read_recent_lines(self.phase_mark)
            .into_iter()
            .find(|line| line.starts_with("MOUNTAIN_TIMING "));
        match line {
            Some(line) => {
                let result = verify_resident_timing(&line);
                self.record_marker(
                    "mountain_timing",
                    if result.is_ok() { "pass" } else { "fail" },
                    Some(line),
                );
                result
            }
            None => {
                self.record_marker("mountain_timing", "missing", None);
                Ok(())
            }
        }
    }

    fn verify_mountain_ui_status(&mut self, args: &Value) -> Result<()> {
        let pattern = Regex::new(r"^MOUNTAIN_STATUS adopted=[01] composed=[01] tx_drop=[0-9]+$")?;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let line = self.console.command_wait_regex(
                "MOUNTAINSTATUS",
                &pattern,
                Duration::from_secs(5),
            )?;
            let values = line.as_deref().and_then(mountain_ui_status_values);
            if let Some((true, true, tx_drop)) = values {
                let lines = self.console.read_recent_lines(self.phase_mark);
                for (checkpoint, prefix) in [
                    ("mountain_validated", "MOUNTAIN_STREAM status=validated"),
                    ("mountain_rendered", "MOUNTAIN_STREAM status=rendered"),
                ] {
                    let direct = lines.iter().find(|entry| entry.starts_with(prefix));
                    self.record_marker(
                        checkpoint,
                        if direct.is_some() { "pass" } else { "inferred" },
                        direct.cloned().or_else(|| line.clone()),
                    );
                }
                self.state_lines.push(format!(
                    "mountain_ui_status cycle={} tx_drop={tx_drop}",
                    self.active_cycle.unwrap_or_default()
                ));
                self.record_marker(
                    super::arg_str(args, "checkpoint").unwrap_or("mountain_ui_status"),
                    "pass",
                    line,
                );
                break Ok(());
            }
            if Instant::now() >= deadline {
                self.record_marker("mountain_ui_status", "fail", line.clone());
                break Err(anyhow!(
                    "Mountain UI did not adopt and compose within 20s: {}",
                    line.as_deref().unwrap_or("<no response>")
                ));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}
