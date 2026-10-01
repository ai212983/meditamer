use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

use super::line_checks::verify_cycle_psram;
use super::report::{parse_psram_line, PsramSample};
use super::{AssetResidencyRuntime, PSRAM_TIMEOUT, STATE_PROBE_TIMEOUT};
use crate::serial_console::AckStatus;

impl AssetResidencyRuntime<'_> {
    pub(super) fn dispatch_state_actions(
        &mut self,
        action: &str,
        args: &Value,
    ) -> Option<Result<()>> {
        match action {
            "psram_snapshot" => Some(self.psram_snapshot(args)),
            "assert_no_fault" => Some(self.assert_no_fault()),
            "state_command" => Some(self.state_command(args)),
            "cleanup_upload_off" => Some(self.cleanup_upload_off()),
            _ => None,
        }
    }

    fn observe_state_line(&mut self, line: &str) -> (bool, Option<String>) {
        self.state_lines.push(line.to_string());
        let ready = line.contains("ready=true");
        let upload = if line.contains("upload=on") {
            Some("on".to_string())
        } else if line.contains("upload=off") {
            Some("off".to_string())
        } else {
            None
        };
        (ready, upload)
    }

    fn probe_once(&mut self) -> Result<Option<String>> {
        let mark = self.console.mark();
        self.console.send_line("STATE GET")?;
        let pattern = Regex::new(r"^STATE ")?;
        self.console
            .wait_for_regex_since(mark, &pattern, STATE_PROBE_TIMEOUT)
    }

    fn snapshot_once(&mut self, label: &str) -> Result<PsramSample> {
        let mark = self.console.mark();
        self.console.send_line("PSRAM")?;
        let pattern = Regex::new(r"^PSRAM(\s|$)")?;
        let line = self
            .console
            .wait_for_regex_since(mark, &pattern, PSRAM_TIMEOUT)?
            .ok_or_else(|| anyhow!("missing PSRAM response for snapshot `{label}`"))?;
        parse_psram_line(label, &line)
    }

    pub(super) fn state_probe_impl(
        &mut self,
        args: &Value,
    ) -> Result<(bool, Option<String>, Option<String>)> {
        let line = self.probe_once()?;
        let (ready, upload, line) = match &line {
            Some(line) => {
                let (ready, upload) = self.observe_state_line(line);
                (ready, upload, Some(line.clone()))
            }
            None => (false, None, None),
        };
        if let Some(expected) = super::arg_str(args, "expect_upload") {
            if upload.as_deref() != Some(expected) {
                if let Some(checkpoint) = super::arg_str(args, "checkpoint") {
                    self.record_marker(checkpoint, "fail", line.clone());
                }
                return Err(anyhow!(
                    "STATE upload mismatch: expected `{expected}`, observed `{}`",
                    line.as_deref().unwrap_or("<no response>")
                ));
            }
        }
        if args.get("expect_ready").and_then(Value::as_bool) == Some(true) && !ready {
            return Err(anyhow!(
                "device not ready: {}",
                line.as_deref().unwrap_or("<no response>")
            ));
        }
        if let Some(checkpoint) = super::arg_str(args, "checkpoint") {
            if line.is_some() {
                let elapsed = self.elapsed_ms();
                let name = self.checkpoint_name(checkpoint);
                let entry = self.checkpoints.entry(name).or_default();
                entry.status = "pass".to_string();
                entry.elapsed_ms = Some(elapsed);
                entry.marker_line = line.clone();
            }
        }
        Ok((ready, upload, line))
    }

    fn psram_snapshot(&mut self, args: &Value) -> Result<()> {
        let label = super::arg_str(args, "label")
            .ok_or_else(|| anyhow!("psram_snapshot requires label"))?;
        let checkpoint = super::arg_str(args, "checkpoint").unwrap_or(label);
        let sample = self.snapshot_once(label)?;
        let checked = if self.active_cycle.is_some() {
            verify_cycle_psram(&sample)
        } else {
            Ok(())
        };
        let elapsed = self.elapsed_ms();
        let name = self.checkpoint_name(checkpoint);
        let entry = self.checkpoints.entry(name).or_default();
        entry.status = if checked.is_ok() { "pass" } else { "fail" }.to_string();
        entry.elapsed_ms = Some(elapsed);
        entry.psram = Some(sample);
        checked
    }

    fn assert_no_fault(&mut self) -> Result<()> {
        self.console.settle(100)?;
        let fault = self
            .console
            .read_recent_lines(self.run_mark)
            .into_iter()
            .find(|line| {
                line.starts_with("BOOT_RESET ")
                    || line.starts_with("PANIC_CRUMB hit=1")
                    || line.starts_with("FIRST_FAULT hit=1")
                    || line.starts_with("STALL_RECORD hit=1")
                    || line.contains("Guru Meditation")
            });
        self.record_marker(
            "no_fault",
            if fault.is_some() { "fail" } else { "pass" },
            fault.clone(),
        );
        if let Some(line) = fault {
            return Err(anyhow!("device reset or fault during reentry run: {line}"));
        }
        Ok(())
    }

    fn state_command(&mut self, args: &Value) -> Result<()> {
        let upload = super::arg_str(args, "upload")
            .ok_or_else(|| anyhow!("state_command requires upload"))?;
        if !matches!(upload, "on" | "off") {
            return Err(anyhow!(
                "state_command requires upload=on|off, got `{upload}`"
            ));
        }
        let command = format!("STATE SET upload={upload}");
        let mark = self.console.mark();
        self.console.send_line(&command)?;
        let (status, line) = self
            .console
            .wait_ack_since(mark, "STATE", STATE_PROBE_TIMEOUT)?;
        match status {
            AckStatus::Ok => {
                if let Some(line) = line {
                    self.state_lines.push(line);
                }
                Ok(())
            }
            AckStatus::None => Err(anyhow!("`{command}` timed out without acknowledgment")),
            AckStatus::Busy | AckStatus::Err => Err(anyhow!(
                "`{command}` failed: {}",
                line.unwrap_or_else(|| "missing response".to_string())
            )),
        }
    }

    fn cleanup_upload_off(&mut self) -> Result<()> {
        if let Ok(Some(line)) = self.probe_once() {
            let (_, upload) = self.observe_state_line(&line);
            if upload.as_deref() == Some("on") {
                let mark = self.console.mark();
                if self.console.send_line("STATE SET upload=off").is_ok() {
                    if let Ok((_, Some(ack))) =
                        self.console
                            .wait_ack_since(mark, "STATE", STATE_PROBE_TIMEOUT)
                    {
                        self.state_lines.push(ack);
                    }
                }
            }
        }
        if let Ok(Some(line)) = self.probe_once() {
            let (_, upload) = self.observe_state_line(&line);
            if upload.as_deref() == Some("off") {
                let elapsed = self.elapsed_ms();
                let entry = self
                    .checkpoints
                    .entry("state_upload_off".to_string())
                    .or_default();
                entry.status = "pass".to_string();
                entry.elapsed_ms = Some(elapsed);
                entry.marker_line = Some(line);
            }
        }
        if let Ok(sample) = self.snapshot_once("upload_off") {
            let elapsed = self.elapsed_ms();
            let entry = self
                .checkpoints
                .entry("upload_off".to_string())
                .or_default();
            entry.status = "pass".to_string();
            entry.elapsed_ms = Some(elapsed);
            entry.psram = Some(sample);
        }
        Ok(())
    }
}
