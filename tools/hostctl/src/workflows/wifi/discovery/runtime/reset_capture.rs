use super::super::WifiDiscoveryRuntime;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::time::{Duration, Instant};

impl WifiDiscoveryRuntime<'_> {
    pub(super) fn handle_capture_reset_trace(&mut self, context: &Value) -> Result<()> {
        let timeout_secs = context["retain_reset_capture_secs"]
            .as_u64()
            .ok_or_else(|| anyhow!("retain_reset_capture_secs is missing"))?;
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut mark = self.capture_start_mark;
        let mut boot_seen = false;
        let mut fault_seen = false;
        let mut task_pointer_seen = false;
        self.logger.info(format!(
            "reset_capture: retaining same serial connection for up to {timeout_secs}s"
        ));

        loop {
            for line in self.console.read_recent_lines(mark) {
                if line.starts_with("BOOT_RESET reason=") {
                    boot_seen = true;
                } else if boot_seen && line.starts_with("FIRST_FAULT ") {
                    fault_seen = true;
                } else if boot_seen && line.starts_with("TASK_POINTER_TRACE ") {
                    task_pointer_seen = true;
                } else if boot_seen && line.starts_with("STALL_RECORD ") {
                    self.logger.info(format!(
                        "reset_capture: boot_record_complete first_fault={} task_pointer_trace={}",
                        fault_seen, task_pointer_seen
                    ));
                    return Ok(());
                }
            }
            mark = self.console.mark();
            if Instant::now() >= deadline {
                self.logger.info(format!(
                    "reset_capture: timeout boot_seen={} first_fault={} task_pointer_trace={}",
                    boot_seen, fault_seen, task_pointer_seen
                ));
                return Ok(());
            }
            if let Err(err) = self.console.poll_once() {
                self.logger
                    .info(format!("reset_capture: serial read error: {err}"));
                return Ok(());
            }
        }
    }

    pub(super) fn handle_fail_captured(&mut self, context: &Value) -> Result<()> {
        let original = context["discovery_error"]["message"]
            .as_str()
            .unwrap_or("wifi discovery debug failed");
        Err(anyhow!("{original}"))
    }
}
