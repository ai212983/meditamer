use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

use crate::{
    scenarios::WorkflowRuntime,
    workflows::wifi::common::{
        ctx_get_string, ctx_get_u32, ctx_set_bool, ctx_set_string, ctx_set_u32,
        detect_panic_signal, extract_context_window, fmt_min, netcfg_set_payload, preflight,
        wait_net_ack,
    },
};

use super::{
    probe::{ProbeRoundState, RoundSample, WifiMetricsScanCounters},
    profile::recommended_round_timeout_ms,
    WifiDiscoveryRuntime,
};

const MAX_EXPECTED_SOFT_RESET_RECOVERIES_PER_ROUND: u32 = 1;
const FORCE_STOP_SETTLE_MS: u64 = 300;
const ACK_LOSS_RECOVERY_SETTLE_MS: u64 = 200;

impl WorkflowRuntime for WifiDiscoveryRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "start_run" => self.handle_start_run(context),
            "net_apply_config" => self.handle_net_apply_config(),
            "probe_round" => self.handle_probe_round(context),
            "evaluate_results" => self.handle_evaluate_results(context),
            "print_summary" => self.handle_print_summary(),
            "fail_run" => self.handle_fail_run(context),
            _ => Err(anyhow!("unknown workflow action: {action}")),
        }
    }
}

include!("runtime/control.rs");
include!("runtime/round.rs");
include!("runtime/summary.rs");

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;
