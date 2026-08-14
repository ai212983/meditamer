use std::{fs, time::Duration};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use super::BlePhase1sRuntime;
use crate::{
    scenarios::WorkflowRuntime,
    workflows::wifi::common::{ctx_get_u32, preflight as serial_preflight, wait_net_ack},
};

impl WorkflowRuntime for BlePhase1sRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, _context: &mut Value) -> Result<()> {
        match action {
            "await_ready" => self.await_phase1s_identity(),
            "capture_owner_identity" => self.capture_initial_owner_identity(),
            "close_listener" => wait_net_ack(&mut self.console, "NET LISTENER OFF"),
            "await_sd_ready" => self.await_sd_ready(),
            "drain_serial_backlog" => serial_preflight(&mut self.console),
            "apply_network_config" => self.apply_network_config_once(),
            "verify_network_config" => self.verify_applied_network_config(),
            "stop_network" => wait_net_ack(&mut self.console, "NET STOP"),
            "await_network_idle" => self.wait_network_idle(),
            "start_network" => wait_net_ack(&mut self.console, "NET START"),
            "await_network_ready" => self
                .wait_network_state(false, Duration::from_secs(190))
                .map(|_| ()),
            "open_listener" => wait_net_ack(&mut self.console, "NET LISTENER ON"),
            "await_listener_ready" => {
                let ip = self.wait_network_state(true, Duration::from_secs(30))?;
                self.logger.info(format!(
                    "Phase 1S post-reset network provisioning passed at {ip}"
                ));
                Ok(())
            }
            "verify_provisioning" => self.verify_post_reset_provisioning(),
            "restore_after_known_failure" => self.restore_after_known_failure(),
            "write_report" => {
                let report = self.finish_report()?;
                fs::write(&self.report_path, serde_json::to_vec_pretty(&report)?)?;
                self.logger.info(format!(
                    "Phase 1S evidence report written: cycles={} report={}",
                    report.completed_cycles,
                    self.report_path.display()
                ));
                Ok(())
            }
            "write_failure_report" => {
                let report = self.finish_failure_report();
                fs::write(&self.report_path, serde_json::to_vec_pretty(&report)?)?;
                self.logger.info(format!(
                    "Phase 1S failure evidence written: stage={} ownership_known={} report={}",
                    report.failure_stage.as_deref().unwrap_or("unknown"),
                    report.ownership_known.unwrap_or(false),
                    self.report_path.display()
                ));
                Ok(())
            }
            "fail_ble_window" => bail!(
                "Phase 1S BLE window failed: {}",
                self.failure_reason
                    .as_deref()
                    .unwrap_or("unclassified BLE lifecycle failure")
            ),
            "fail_final_metrics" => bail!(
                "Phase 1S final metrics failed: {}",
                self.failure_reason
                    .as_deref()
                    .unwrap_or("unclassified final metrics failure")
            ),
            "assert_stack_floors" => self.assert_stack_floors(),
            other => Err(anyhow!("unsupported ble-phase1s action: {other}")),
        }
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        if action == "init_run" {
            return Ok(Some(json!({ "cycle_count": self.cycles })));
        }
        if action == "prepare_off_window" {
            let cycle = ctx_get_u32(context, "cycle_index")? + 1;
            return Ok(Some(self.prepare_off_window_outcome(cycle)));
        }
        if action == "run_ble_window" {
            return self.run_ble_window().map(Some);
        }
        if action == "restore_and_complete_cycle" {
            return Ok(Some(self.restore_and_complete_cycle_outcome()));
        }
        if action == "collect_stack_metrics" {
            return Ok(Some(self.collect_stack_metrics_outcome()));
        }
        self.invoke(action, args, context)?;
        Ok(None)
    }
}
