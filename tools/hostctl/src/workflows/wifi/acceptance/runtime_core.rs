//! Workflow runtime for the Wi-Fi acceptance run: startup, network bring-up, health, and diagnostics.
//!
mod diag;
mod health;
mod network;
mod start;

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::scenarios::WorkflowRuntime;

use super::{wait_ready::wait_state_progress, WifiAcceptanceRuntime};

impl WorkflowRuntime for WifiAcceptanceRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        self.capture_mem_diag_lines()?;
        let result = match action {
            "prepare_payload" => self.handle_prepare_payload(),
            "wait_runtime_ready" => self.handle_wait_runtime_ready(),
            "boot_discovery_gate" => self.handle_boot_discovery_gate(),
            "start_run" => {
                let _ = context;
                self.handle_start_run()
            }
            "prepare_measurement" => self.handle_prepare_measurement(),
            "net_apply_config" => self.handle_net_apply_config(),
            "net_start" => self.handle_net_start(),
            "net_wait_state" => {
                wait_state_progress(&mut self.console, self.policy.connect_timeout_ms)
            }
            "init_wait_ready_recovery" => {
                let _ = self.handle_init_wait_ready_recovery()?;
                Ok(())
            }
            "net_wait_ready_once" => {
                let _ = self.handle_net_wait_ready_once(context)?;
                Ok(())
            }
            "init_upload_attempt" => {
                let _ = context;
                self.handle_init_upload_attempt()
            }
            "net_upload_once" => {
                let _ = self.handle_net_upload_once(context)?;
                Ok(())
            }
            "net_verify_once" => self.handle_net_verify_once(context),
            "assert_upload_metrics" => {
                let _ = self.handle_assert_upload_metrics()?;
                Ok(())
            }
            "net_collect_diag" => self.handle_net_collect_diag(),
            "net_recover_once" => self.handle_net_recover_once(),
            "fail_upload" | "net_fail" => self.handle_fail_upload(context),
            "finalize_cycle" => self.handle_finalize_cycle(context),
            "assert_runtime_health" => self.handle_assert_runtime_health(),
            "print_summary" => self.handle_print_summary(),
            _ => Err(anyhow!("unknown workflow action: {action}")),
        };
        self.capture_mem_diag_lines()?;
        result
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        match action {
            "start_run" => {
                self.capture_mem_diag_lines()?;
                self.handle_start_run()?;
                self.capture_mem_diag_lines()?;
                Ok(Some(self.build_start_run_result()))
            }
            "init_upload_attempt" => {
                self.capture_mem_diag_lines()?;
                self.handle_init_upload_attempt()?;
                self.capture_mem_diag_lines()?;
                Ok(Some(self.build_init_upload_attempt_result()))
            }
            "init_wait_ready_recovery" => {
                self.capture_mem_diag_lines()?;
                let result = self.handle_init_wait_ready_recovery()?;
                self.capture_mem_diag_lines()?;
                Ok(Some(result))
            }
            "net_wait_ready_once" => {
                self.capture_mem_diag_lines()?;
                let result = self.handle_net_wait_ready_once(context)?;
                self.capture_mem_diag_lines()?;
                Ok(Some(result))
            }
            "net_upload_once" => {
                self.capture_mem_diag_lines()?;
                let result = self.handle_net_upload_once(context)?;
                self.capture_mem_diag_lines()?;
                Ok(Some(result))
            }
            "assert_upload_metrics" => {
                self.capture_mem_diag_lines()?;
                let result = self.handle_assert_upload_metrics()?;
                self.capture_mem_diag_lines()?;
                Ok(Some(result))
            }
            _ => {
                self.invoke(action, args, context)?;
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
#[path = "runtime_core/tests.rs"]
mod tests;
