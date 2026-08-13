//! Workflow runtime for the Wi-Fi discovery sweep.
//!
//! [`control`] owns the device control channel and its recovery paths, [`round`]
//! runs one probe round, and [`summary`] records and reports results. This file
//! maps workflow actions onto them.

mod control;
mod round;
mod summary;
#[cfg(test)]
mod tests;

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::WifiDiscoveryRuntime;
use crate::scenarios::WorkflowRuntime;

impl WorkflowRuntime for WifiDiscoveryRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "start_run" => {
                let _ = context;
                self.handle_start_run()
            }
            "net_apply_config" => self.handle_net_apply_config(),
            "probe_round" => {
                let _ = self.handle_probe_round(context)?;
                Ok(())
            }
            "evaluate_results" => {
                let _ = self.handle_evaluate_results();
                Ok(())
            }
            "print_summary" => self.handle_print_summary(),
            "fail_run" => self.handle_fail_run(context),
            _ => Err(anyhow!("unknown workflow action: {action}")),
        }
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        match action {
            "start_run" => {
                self.handle_start_run()?;
                Ok(Some(self.build_start_run_result()))
            }
            "probe_round" => Ok(Some(self.handle_probe_round(context)?)),
            "evaluate_results" => Ok(Some(self.handle_evaluate_results())),
            _ => {
                self.invoke(action, args, context)?;
                Ok(None)
            }
        }
    }
}
