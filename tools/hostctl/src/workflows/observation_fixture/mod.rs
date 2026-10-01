//! Expiring typed observation fixtures over UART; YAML owns run and failure flow.

mod lifecycle;
mod periodic;
#[cfg(test)]
mod tests;
pub(crate) mod wire;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use regex::Regex;
use serde_json::{json, Value};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::SerialConsole,
    workflows::wifi::common::acquire_port_lock,
};
use wire::{parse_result, verify_sample, FixtureResult};

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum FixtureProvider {
    Battery,
    Bme688,
    Shtc3,
    Adc,
}

impl FixtureProvider {
    pub(crate) fn id(self) -> u64 {
        match self {
            Self::Battery | Self::Adc => 2,
            Self::Bme688 | Self::Shtc3 => 1,
        }
    }
    pub(crate) fn command(self) -> &'static str {
        match self {
            Self::Battery => "BATTERY",
            Self::Bme688 => "BME688",
            Self::Shtc3 => "SHTC3",
            Self::Adc => "ADC",
        }
    }
    fn fields(self) -> u64 {
        match self {
            Self::Battery => 1,
            Self::Bme688 | Self::Shtc3 | Self::Adc => 3,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ObservationFixtureOptions {
    pub provider: FixtureProvider,
    pub period_ms: Option<u32>,
    pub panel_cycle: bool,
    pub cancel_after_samples: Option<usize>,
    pub request_id: Option<u64>,
    pub validity_ms: u32,
    pub timeout_ms: u64,
    pub observe_ms: u64,
    pub output_path: Option<PathBuf>,
}

fn validate_options(
    id: u64,
    validity_ms: u32,
    timeout_ms: u64,
    observe_ms: u64,
    periodic: bool,
) -> Result<()> {
    if id == 0 {
        bail!("request-id must be nonzero");
    }
    let validity_limit = if periodic { 900_000 } else { 300_000 };
    if !(1..=validity_limit).contains(&validity_ms) {
        bail!("validity-ms must be in 1..={validity_limit}");
    }
    if timeout_ms == 0
        || Instant::now()
            .checked_add(Duration::from_millis(timeout_ms))
            .is_none()
    {
        bail!("timeout-ms must be a positive representable host deadline");
    }
    if Instant::now()
        .checked_add(Duration::from_millis(observe_ms))
        .is_none()
    {
        bail!("observe-ms must be a representable host capture duration");
    }
    Ok(())
}

pub(crate) fn generated_request_id() -> Result<u64> {
    u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos())
        .context("host clock cannot produce a u64 observation request ID")
}

fn report_path(log: &Path) -> PathBuf {
    let mut path = log.as_os_str().to_os_string();
    path.push(".report.json");
    PathBuf::from(path)
}

struct ObservationFixtureRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    id: u64,
    provider: FixtureProvider,
    period_ms: Option<u32>,
    cancel_after_samples: Option<usize>,
    periodic_evidence: Option<periodic::Evidence>,
    panel_cycle: bool,
    lifecycle_evidence: Option<lifecycle::Evidence>,
    preflight_mark: usize,
    validity_ms: u32,
    timeout: Duration,
    observe: Duration,
    log_path: PathBuf,
    command_mark: usize,
    result: Option<FixtureResult>,
}

impl ObservationFixtureRuntime<'_> {
    fn submit_request(&mut self) -> Result<()> {
        self.command_mark = self.console.mark();
        let mut command = match self.period_ms {
            Some(period) => format!(
                "OBSPER {} {} {} {}",
                self.provider.command(),
                self.id,
                period,
                self.validity_ms
            ),
            None => format!(
                "OBSFIX {} {} {}",
                self.provider.command(),
                self.id,
                self.validity_ms
            ),
        };
        if self.panel_cycle {
            command.push_str(" REPAINT");
        }
        self.console.send_line(&command)?;
        Ok(())
    }

    fn write_report(&self, context: &Value) -> Result<()> {
        let report = json!({
            "request_id": self.id,
            "provider": self.provider.id(),
            "provider_kind": self.provider.command(),
            "validity_ms": self.validity_ms,
            "timeout_ms": self.timeout.as_millis(),
            "observe_ms": self.observe.as_millis(),
            "sample_verified": context["sample_verified"].as_bool().unwrap_or(false),
            "observation_completed": context["observation_completed"].as_bool().unwrap_or(false),
            "observed_bytes": context.get("observed_bytes"),
            "passed": context["fixture_passed"].as_bool().unwrap_or(false),
            "result": self.result,
            "period_ms": self.period_ms,
            "cancel_after_samples": self.cancel_after_samples,
            "periodic": self.periodic_evidence,
            "panel_cycle": self.panel_cycle,
            "lifecycle": self.lifecycle_evidence,
            "error": context.get("fixture_error"),
            "cleanup": if self.period_ms.is_some() { "provider restores latest live demand at expiry or correlated cancellation; no retry sent" } else { "one-shot expires in firmware; no cancellation or retry sent" },
        });
        std::fs::write(
            report_path(&self.log_path),
            serde_json::to_vec_pretty(&report)?,
        )?;
        Ok(())
    }
}

impl WorkflowRuntime for ObservationFixtureRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "check_ready" => {
                // Some USB adapters reset on reopen. Wait for either a live
                // PONG or the boot readiness event, then let YAML decide whether
                // a second read-only PING is needed. Never retry OBSFIX here.
                let line = self.console.command_wait_regex(
                    "PING",
                    &Regex::new(r"^(?:PONG|RUNTIME_READY app_state=ready display=ready)$")?,
                    self.timeout,
                )?.ok_or_else(|| anyhow!("device did not answer PING or reach runtime readiness; no fixture command sent"))?;
                context["ping_confirmed"] = json!(line == "PONG");
            }
            "confirm_ready" => {
                self.console
                    .command_wait_regex("PING", &Regex::new(r"^PONG$")?, self.timeout)?
                    .ok_or_else(|| anyhow!("device reached runtime readiness but did not answer PING; no fixture command sent"))?;
            }
            "await_periodic_samples" => self.await_periodic_samples()?,
            "cancel_periodic" => self.console.send_line(&format!(
                "OBSPER {} {} CANCEL",
                self.provider.command(),
                self.id
            ))?,
            "await_periodic_result" => self.await_periodic_result()?,
            "observe_window" => {
                let bytes = self.console.capture_raw_for(self.observe)?;
                context["observed_bytes"] = json!(bytes.len());
                context["observation_completed"] = json!(true);
            }
            "submit_request" => self.submit_request()?,
            "await_result" => {
                let pattern = Regex::new(&format!(r"^OBSFIX RESULT id={}(?: |$)", self.id))?;
                let line = self
                    .console
                    .wait_for_regex_since(self.command_mark, &pattern, self.timeout)?
                    .ok_or_else(|| anyhow!(
                        "timed out waiting for OBSFIX RESULT id={}; admission may have succeeded. \
                         If admitted, the request remains eligible only until its firmware deadline \
                         (requested validity {} ms); no cancellation or retry was sent",
                        self.id,
                        self.validity_ms
                    ))?;
                self.result = Some(parse_result(&line, self.provider)?);
            }
            "write_report" => self.write_report(context)?,
            "print_summary" => self.logger.info(format!(
                "{} observation fixture passed: id={} report={}",
                self.provider.command(),
                self.id,
                report_path(&self.log_path).display()
            )),
            "fail_fixture" => bail!(
                "observation fixture failed: {}; report={}",
                context["fixture_error"]["message"]
                    .as_str()
                    .unwrap_or("missing successful sample evidence"),
                report_path(&self.log_path).display()
            ),
            other if other.starts_with("lifecycle_") => self.invoke_lifecycle(other)?,
            other => bail!("unsupported observation-fixture action: {other}"),
        }
        Ok(())
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        if action == "assert_sample" {
            verify_sample(
                self.result
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing OBSFIX result"))?,
                self.id,
                self.validity_ms,
                self.provider,
            )?;
            return Ok(Some(json!({ "sample_verified": true })));
        }
        self.invoke(action, args, context)?;
        Ok(None)
    }
}

pub fn run_observation_fixture(logger: &mut Logger, opts: ObservationFixtureOptions) -> Result<()> {
    let id = match opts.request_id {
        Some(id) => id,
        None => generated_request_id()?,
    };
    lifecycle::validate(&opts, id)?;
    periodic::validate(opts.period_ms, opts.cancel_after_samples, opts.validity_ms)?;
    validate_options(
        id,
        opts.validity_ms,
        opts.timeout_ms,
        opts.observe_ms,
        opts.period_ms.is_some(),
    )?;
    let log_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(format!(
            "logs/observation_fixture_{}_{}.log",
            Local::now().format("%Y%m%d_%H%M%S"),
            id
        ))
    });
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115_200)?;
    let _port_lock = acquire_port_lock(&port)?;
    ensure_parent_dir(&log_path)?;
    let console = SerialConsole::open(&port, baud, Some(&log_path))?;
    let workflow = load_workflow(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        if opts.panel_cycle {
            "scenarios/observation-panel-cycle.sw.yaml"
        } else if opts.period_ms.is_some() {
            "scenarios/observation-periodic.sw.yaml"
        } else {
            "scenarios/observation-fixture.sw.yaml"
        },
    ))?;
    let mut runtime = ObservationFixtureRuntime {
        logger,
        console,
        id,
        provider: opts.provider,
        period_ms: opts.period_ms,
        cancel_after_samples: opts.cancel_after_samples,
        periodic_evidence: None,
        panel_cycle: opts.panel_cycle,
        lifecycle_evidence: None,
        preflight_mark: 0,
        validity_ms: opts.validity_ms,
        timeout: Duration::from_millis(opts.timeout_ms),
        observe: Duration::from_millis(opts.observe_ms),
        log_path,
        command_mark: 0,
        result: None,
    };
    execute_workflow(
        &workflow,
        &mut runtime,
        &json!({ "fixture_passed": false, "cancel_requested": opts.cancel_after_samples.is_some() }),
    )?;
    Ok(())
}
