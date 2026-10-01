//! Workflow runtime for the Wi-Fi acceptance run: startup, network bring-up, health, and diagnostics.
//!
mod diag;
mod health;
mod network;
mod s3;
mod start;

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::{scenarios::WorkflowRuntime, serial_console::AckStatus};

use super::{wait_ready::wait_state_progress, WifiAcceptanceRuntime};

pub(super) fn set_discovery_log_domain(
    args: &Value,
    send: impl FnOnce(&str) -> Result<(AckStatus, Option<String>)>,
) -> Result<()> {
    let domain = args["domain"]
        .as_str()
        .filter(|domain| matches!(*domain, "WIFI" | "REASSOC"))
        .ok_or_else(|| anyhow!("discovery log domain must be WIFI or REASSOC"))?;
    let command = format!("TELEMSET {domain} ON");
    let (status, line) = send(&command)?;
    if status != AckStatus::Ok {
        return Err(anyhow!(
            "discovery logging prerequisite failed for {domain}: {}",
            line.as_deref()
                .unwrap_or("missing TELEMSET acknowledgement")
        ));
    }
    Ok(())
}

impl WifiAcceptanceRuntime<'_> {
    fn dispatch_action(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        if self.dispatch_start_phase(action, args, context)? {
            return Ok(());
        }
        if self.dispatch_s3(action, args, context)? {
            return Ok(());
        }
        if self.dispatch_network(action, args, context)? {
            return Ok(());
        }
        if self.dispatch_upload_cycle(action, args, context)? {
            return Ok(());
        }
        Err(anyhow!("unknown workflow action: {action}"))
    }

    fn dispatch_start_phase(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<bool> {
        match action {
            "prepare_payload" => self.handle_prepare_payload()?,
            "wait_runtime_ready" => self.handle_wait_runtime_ready()?,
            "set_discovery_log_domain" => self.handle_set_discovery_log_domain(args)?,
            "boot_discovery_gate" => self.handle_boot_discovery_gate()?,
            "start_run" => {
                let _ = context;
                self.handle_start_run()?
            }
            "prepare_measurement" => self.handle_prepare_measurement()?,
            "assert_runtime_health" => self.handle_assert_runtime_health()?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn dispatch_s3(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<bool> {
        let _ = (args, context);
        match action {
            "s3_runtime_health" => self.handle_s3_runtime_health()?,
            "s3_network_health" => self.handle_s3_network_health()?,
            "s3_stop" => self.handle_s3_stop()?,
            "s3_start" => self.handle_s3_start()?,
            "s3_configure" => self.handle_s3_configure()?,
            "s3_assert_discovery" => self.handle_s3_assert_discovery()?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn dispatch_network(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<bool> {
        let _ = args;
        match action {
            "net_apply_config" => self.handle_net_apply_config()?,
            "net_start" => self.handle_net_start()?,
            "net_wait_state" => {
                wait_state_progress(&mut self.console, self.policy.connect_timeout_ms)?;
            }
            "init_wait_ready_recovery" => {
                let _ = self.handle_init_wait_ready_recovery()?;
            }
            "net_wait_ready_once" => {
                let _ = self.handle_net_wait_ready_once(context)?;
            }
            "init_upload_attempt" => {
                let _ = context;
                self.handle_init_upload_attempt()?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn dispatch_upload_cycle(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<bool> {
        let _ = args;
        match action {
            "net_upload_once" => {
                let _ = self.handle_net_upload_once(context)?;
            }
            "net_verify_once" => self.handle_net_verify_once(context)?,
            "assert_upload_metrics" => {
                let _ = self.handle_assert_upload_metrics()?;
            }
            "net_collect_diag" => self.handle_net_collect_diag()?,
            "net_recover_once" => self.handle_net_recover_once()?,
            "fail_upload" | "net_fail" => self.handle_fail_upload(context)?,
            "finalize_cycle" => self.handle_finalize_cycle(context)?,
            "print_summary" => self.handle_print_summary()?,
            _ => return Ok(false),
        }
        Ok(true)
    }
}

impl WorkflowRuntime for WifiAcceptanceRuntime<'_> {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        self.capture_mem_diag_lines()?;
        let result = self.dispatch_action(action, args, context);
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
