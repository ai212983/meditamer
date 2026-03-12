use std::{
    fs, thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use regex::Regex;
use reqwest::StatusCode;
use serde_json::Value;

use crate::{
    env_utils,
    logging::ensure_parent_dir,
    scenarios::WorkflowRuntime,
    serial_console::AckStatus,
    workflows::wifi::common::{
        ctx_get_u32, ctx_set_bool, ctx_set_string, ctx_set_u32, detect_panic_signal,
        extract_context_window, fmt_min, is_ready, netcfg_set_payload, query_net_status,
        wait_net_ack, NetStatus, PanicSignal,
    },
};

use super::{
    wait_ready::{wait_ready, wait_state_progress},
    WifiAcceptanceRuntime,
};

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

include!("runtime_core/diag.rs");
include!("runtime_core/start.rs");
include!("runtime_core/network.rs");
include!("runtime_core/health.rs");

#[cfg(test)]
#[path = "runtime_core/tests.rs"]
mod tests;
