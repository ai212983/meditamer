use std::{
    collections::HashMap,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

use crate::{logging::Logger, scenarios::WorkflowRuntime, serial_console::SerialConsole};

use super::{
    io::{run_raw_expect_pattern, run_step, wait_for_pattern, wait_for_sd_result},
    templates::{optional_arg_u32, required_arg_str, resolve_templates},
};

mod cutover;

pub(super) struct SdcardScenarioRuntime<'a> {
    logger: &'a mut Logger,
    console: &'a mut SerialConsole,
    vars: HashMap<String, String>,
    sdwait_timeout_ms: u32,
    burst_mark: Option<usize>,
    scenario_mark: usize,
}

impl<'a> SdcardScenarioRuntime<'a> {
    pub(super) fn new(
        logger: &'a mut Logger,
        console: &'a mut SerialConsole,
        vars: HashMap<String, String>,
        sdwait_timeout_ms: u32,
    ) -> Self {
        let scenario_mark = console.mark();
        Self {
            logger,
            console,
            vars,
            sdwait_timeout_ms,
            burst_mark: None,
            scenario_mark,
        }
    }

    fn resolve(&self, raw: &str) -> Result<String> {
        resolve_templates(raw, &self.vars)
    }

    fn invoke_run_step(&mut self, args: &Value) -> Result<()> {
        let name = self.resolve(required_arg_str(args, "name", "run_step")?)?;
        let command = self.resolve(required_arg_str(args, "command", "run_step")?)?;
        let ack_tag = required_arg_str(args, "ack_tag", "run_step")?;
        let expected_status = required_arg_str(args, "expected_status", "run_step")?;
        let expected_code = args.get("expected_code").and_then(|value| value.as_str());
        let timeout_ms = optional_arg_u32(args, "timeout_ms").unwrap_or(self.sdwait_timeout_ms);

        let expected_pattern = if let Some(raw_pattern) = args
            .get("expected_pattern")
            .and_then(|value| value.as_str())
        {
            Some(Regex::new(&self.resolve(raw_pattern)?)?)
        } else {
            None
        };

        run_step(
            self.logger,
            self.console,
            &name,
            &command,
            ack_tag,
            expected_status,
            expected_code,
            expected_pattern.as_ref(),
            timeout_ms,
        )
    }

    fn invoke_raw_expect_pattern(&mut self, args: &Value) -> Result<()> {
        let name = self.resolve(required_arg_str(args, "name", "raw_expect_pattern")?)?;
        let command = self.resolve(required_arg_str(args, "command", "raw_expect_pattern")?)?;
        let expected_pattern = Regex::new(&self.resolve(required_arg_str(
            args,
            "expected_pattern",
            "raw_expect_pattern",
        )?)?)?;
        let timeout_ms = optional_arg_u32(args, "timeout_ms").unwrap_or(20_000);

        run_raw_expect_pattern(
            self.logger,
            self.console,
            &name,
            &command,
            &expected_pattern,
            Duration::from_millis(timeout_ms as u64),
        )
    }

    fn invoke_wait_pattern(&mut self, args: &Value) -> Result<()> {
        let name = self.resolve(required_arg_str(args, "name", "wait_pattern")?)?;
        let expected_pattern = Regex::new(&self.resolve(required_arg_str(
            args,
            "expected_pattern",
            "wait_pattern",
        )?)?)?;
        let timeout_ms = optional_arg_u32(args, "timeout_ms").unwrap_or(20_000);
        wait_for_pattern(
            self.logger,
            self.console,
            &name,
            &expected_pattern,
            Duration::from_millis(timeout_ms as u64),
        )
    }

    fn invoke_poll_command_pattern(&mut self, args: &Value) -> Result<()> {
        let name = self.resolve(required_arg_str(args, "name", "poll_command_pattern")?)?;
        let command = self.resolve(required_arg_str(args, "command", "poll_command_pattern")?)?;
        let response_pattern = Regex::new(&self.resolve(required_arg_str(
            args,
            "response_pattern",
            "poll_command_pattern",
        )?)?)?;
        let expected_pattern = Regex::new(&self.resolve(required_arg_str(
            args,
            "expected_pattern",
            "poll_command_pattern",
        )?)?)?;
        let timeout_ms = optional_arg_u32(args, "timeout_ms").unwrap_or(20_000);
        let interval_ms = optional_arg_u32(args, "interval_ms").unwrap_or(100);
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut last_response = None;

        while Instant::now() < deadline {
            let mark = self.console.mark();
            self.console.send_line(&command)?;
            if let Some(line) = self.console.wait_for_regex_since(
                mark,
                &response_pattern,
                Duration::from_secs(1),
            )? {
                if expected_pattern.is_match(&line) {
                    self.logger.info(format!("[PASS] {name}"));
                    return Ok(());
                }
                last_response = Some(line);
            }
            thread::sleep(Duration::from_millis(interval_ms as u64));
        }

        Err(anyhow!(
            "[FAIL] {name}: state did not converge; last response: {}",
            last_response.unwrap_or_else(|| "<none>".to_string())
        ))
    }

    fn invoke_burst_batch_start(&mut self, args: &Value) -> Result<()> {
        let commands = args
            .get("commands")
            .and_then(|value| value.as_array())
            .ok_or_else(|| anyhow!("burst_batch_start requires array argument 'commands'"))?;

        if commands.is_empty() {
            return Err(anyhow!("burst_batch_start requires at least one command"));
        }

        let mark = self.console.mark();
        for command in commands {
            let command = command
                .as_str()
                .ok_or_else(|| anyhow!("burst_batch_start commands must be strings"))?;
            self.console.send_line(&self.resolve(command)?)?;
        }

        self.burst_mark = Some(mark);
        Ok(())
    }

    fn invoke_burst_batch_assert(&mut self, args: &Value) -> Result<()> {
        let name = args
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("burst_sequence")
            .to_string();
        let expected_sdreq_count = optional_arg_u32(args, "expected_sdreq_count").unwrap_or(1);
        let expected_status = args
            .get("expected_status")
            .and_then(|value| value.as_str())
            .unwrap_or("ok");
        let expected_code = args.get("expected_code").and_then(|value| value.as_str());
        let poll_timeout_ms = optional_arg_u32(args, "poll_timeout_ms").unwrap_or(30_000);
        let sdwait_timeout_ms =
            optional_arg_u32(args, "sdwait_timeout_ms").unwrap_or(self.sdwait_timeout_ms);

        let busy_pattern = args
            .get("busy_pattern")
            .and_then(|value| value.as_str())
            .unwrap_or(r"SDFAT(MKDIR|WRITE|APPEND|STAT|READ) BUSY");
        let busy_re = Regex::new(&self.resolve(busy_pattern)?)?;

        let start = self
            .burst_mark
            .ok_or_else(|| anyhow!("burst_batch_assert called before burst_batch_start"))?;

        let sdreq_re = Regex::new(r"^SDREQ id=[0-9]+ op=")?;
        let deadline = Instant::now() + Duration::from_millis(poll_timeout_ms as u64);
        while Instant::now() < deadline {
            self.console.poll_once()?;
            if self.console.count_regex_since(start, &sdreq_re) >= expected_sdreq_count as usize {
                break;
            }
            thread::sleep(Duration::from_millis(150));
        }

        if self.console.count_regex_since(start, &sdreq_re) < expected_sdreq_count as usize {
            return Err(anyhow!(
                "{name}: observed fewer than {expected_sdreq_count} SDREQ lines"
            ));
        }

        let last_line = self
            .console
            .last_regex_since(start, &sdreq_re)
            .ok_or_else(|| anyhow!("{name}: missing SDREQ lines"))?;
        let id_caps = Regex::new(r"id=([0-9]+)")?
            .captures(&last_line)
            .ok_or_else(|| anyhow!("{name}: failed parsing last SDREQ id"))?;
        let req_id = id_caps
            .get(1)
            .ok_or_else(|| anyhow!("{name}: missing SDREQ capture"))?
            .as_str()
            .parse::<u32>()?;
        wait_for_sd_result(
            self.console,
            req_id,
            sdwait_timeout_ms,
            expected_status,
            expected_code,
        )?;

        if self.console.has_regex_since(start, &busy_re) {
            return Err(anyhow!("{name}: burst flow emitted BUSY markers"));
        }

        self.logger.info(format!("[PASS] {name}"));
        self.burst_mark = None;
        Ok(())
    }

    fn invoke_repeat_step(&mut self, args: &Value) -> Result<()> {
        let repetitions = optional_arg_u32(args, "repetitions").unwrap_or(1);
        let step = args
            .get("step")
            .ok_or_else(|| anyhow!("repeat_step requires object argument 'step'"))?;
        for iteration in 1..=repetitions {
            self.vars
                .insert("iteration".to_string(), iteration.to_string());
            self.invoke_run_step(step)?;
        }
        self.vars.remove("iteration");
        Ok(())
    }
}

impl WorkflowRuntime for SdcardScenarioRuntime<'_> {
    fn invoke(&mut self, action: &str, args: &Value, _context: &mut Value) -> Result<()> {
        match action {
            "run_step" => self.invoke_run_step(args),
            "wait_pattern" => self.invoke_wait_pattern(args),
            "poll_command_pattern" => self.invoke_poll_command_pattern(args),
            "raw_expect_pattern" => self.invoke_raw_expect_pattern(args),
            "burst_batch_start" => self.invoke_burst_batch_start(args),
            "burst_batch_assert" => self.invoke_burst_batch_assert(args),
            "repeat_step" => self.invoke_repeat_step(args),
            "cutover_summary" => self.invoke_cutover_summary(args),
            "complete" => Ok(()),
            other => Err(anyhow!("unsupported sdcard workflow action: {other}")),
        }
    }
}
