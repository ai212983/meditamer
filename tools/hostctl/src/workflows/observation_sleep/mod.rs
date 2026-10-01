//! UI-owned Medinote sleep qualification. YAML owns reconnect and failure flow.
mod evidence;
mod periodic;

use crate::{
    env_utils,
    logging::Logger,
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::SerialConsole,
    workflows::wifi::common::acquire_port_lock,
};
use anyhow::{anyhow, bail, Result};
use clap::{Args, ValueEnum};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Mode {
    Deep,
    RejectEnvironment,
    RejectBattery,
}
impl Mode {
    fn wire(self) -> &'static str {
        match self {
            Self::Deep => "DEEP",
            Self::RejectEnvironment => "REJECT_ENV",
            Self::RejectBattery => "REJECT_ADC",
        }
    }
}

#[derive(Debug, Args)]
pub struct Options {
    #[arg(long, value_enum, default_value = "deep")]
    mode: Mode,
    #[arg(long)]
    request_id: Option<u64>,
    #[arg(long, default_value_t = 30_000)]
    validity_ms: u32,
    #[arg(long, default_value_t = 90_000)]
    timeout_ms: u64,
    /// Arm both periodic providers before rejection and verify live recovery.
    #[arg(long)]
    period_ms: Option<u32>,
    #[arg(long, default_value_t = 600_000)]
    periodic_validity_ms: u32,
    /// Directory containing before/after USB captures and the JSON report.
    #[arg(long)]
    output: PathBuf,
}

struct Runtime {
    console: Option<SerialConsole>,
    port: String,
    baud: u32,
    output: PathBuf,
    id: u64,
    mode: Mode,
    validity_ms: u32,
    timeout: Duration,
    deadline: Instant,
    mark: usize,
    before: Vec<String>,
    evidence: Option<Value>,
    periodic: Option<periodic::Setup>,
}

impl Runtime {
    fn remaining(&self) -> Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| anyhow!("sleep fixture deadline expired; no sleep command retry sent"))
    }
    fn console(&mut self) -> Result<&mut SerialConsole> {
        self.console
            .as_mut()
            .ok_or_else(|| anyhow!("serial port is disconnected"))
    }
    fn lines(&self) -> Vec<String> {
        let mut lines = self.before.clone();
        if let Some(console) = &self.console {
            lines.extend(console.read_recent_lines(self.mark));
        }
        lines
    }
    fn await_transition(&mut self) -> Result<()> {
        let pattern = format!("id={}", self.id);
        loop {
            self.remaining()?;
            let read = self.console()?.poll_once();
            let lines = self.lines();
            if lines.iter().any(|l| {
                (l.starts_with("OBSSLEEP ENTER ") || l.starts_with("OBSSLEEP END "))
                    && l.split_whitespace().any(|w| w == pattern)
            }) {
                evidence::transition(&lines, self.id, self.mode)?;
                return Ok(());
            }
            read?;
        }
    }
    fn await_recovery(&mut self) -> Result<()> {
        loop {
            self.remaining()?;
            let read = self.console()?.poll_once();
            if let Some(mut evidence) = evidence::validate(&self.lines(), self.id, self.mode)? {
                read?;
                if let Some(periodic) = self.periodic {
                    let Some(restoration) = periodic.validate(&self.lines(), self.id, &evidence)?
                    else {
                        continue;
                    };
                    evidence["periodic_restoration"] = restoration;
                }
                self.evidence = Some(evidence);
                return Ok(());
            }
            read?;
        }
    }
    fn start_periodic(&mut self, args: &Value) -> Result<()> {
        let provider = args["provider"]
            .as_u64()
            .ok_or_else(|| anyhow!("missing provider"))?;
        let setup = self
            .periodic
            .ok_or_else(|| anyhow!("periodic setup missing"))?;
        let command = setup.command(self.id, provider)?;
        self.console()?.send_line(&command)?;
        Ok(())
    }
    fn await_periodic(&mut self, provider: u64) -> Result<()> {
        let setup = self
            .periodic
            .ok_or_else(|| anyhow!("periodic setup missing"))?;
        loop {
            self.remaining()?;
            self.console()?.poll_once()?;
            if setup.applied(&self.lines(), self.id, provider)? {
                return Ok(());
            }
        }
    }
    fn write_report(&self, context: &Value) -> Result<()> {
        let report = json!({"request_id":self.id, "mode":self.mode.wire(), "port":self.port, "passed":context["passed"].as_bool().unwrap_or(false), "evidence":self.evidence, "error":context.get("error"), "context":context, "injection":"selected acknowledgement fails once after real provider cleanup; not an electrical fault", "command_retries":0, "periodic_setup":self.periodic.map(|p| json!({"interval_ms":p.interval_ms,"validity_ms":p.validity_ms,"environment_id":self.id-2,"battery_id":self.id-1}))});
        std::fs::write(
            self.output.join("report.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        Ok(())
    }
}

impl WorkflowRuntime for Runtime {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "check_ready" => {
                let timeout = self.timeout;
                self.console()?
                    .command_wait_regex("PING", &Regex::new("^PONG$")?, timeout)?
                    .ok_or_else(|| anyhow!("Medinote did not answer PING; no sleep sent"))?;
                self.deadline = Instant::now() + self.timeout;
                self.mark = self.console()?.mark();
            }
            "start_periodic" => self.start_periodic(args)?,
            "await_periodic" => self.await_periodic(
                args["provider"]
                    .as_u64()
                    .ok_or_else(|| anyhow!("missing provider"))?,
            )?,
            "submit" => {
                let command = format!(
                    "OBSSLEEP {} {} {}",
                    self.id,
                    self.validity_ms,
                    self.mode.wire()
                );
                self.console()?.send_line(&command)?;
            }
            "await_transition" => self.await_transition()?,
            "disconnect" => {
                self.before = self.lines();
                self.console.take();
                self.mark = 0;
            }
            "check_disconnected" => {
                self.remaining()?;
                context["disconnected"] = json!(!Path::new(&self.port).exists());
            }
            "reconnect_once" => {
                self.remaining()?;
                match SerialConsole::open_passive(
                    &self.port,
                    self.baud,
                    Some(&self.output.join("after.log")),
                ) {
                    Ok(console) => {
                        self.console = Some(console);
                        context["connected"] = json!(true);
                    }
                    Err(error) => {
                        context["connect_error"] = json!(error.to_string());
                        context["connected"] = json!(false);
                    }
                }
            }
            "poll_interval" => {
                std::thread::sleep(self.remaining()?.min(Duration::from_millis(100)));
            }
            "await_recovery" => self.await_recovery()?,
            "write_report" => self.write_report(context)?,
            "fail" => bail!(
                "sleep qualification failed: {}; report={}",
                context["error"],
                self.output.join("report.json").display()
            ),
            other => bail!("unknown sleep workflow action: {other}"),
        }
        Ok(())
    }
}

pub fn run(logger: &mut Logger, options: Options) -> Result<()> {
    let id = options.request_id.unwrap_or(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
    )?);
    if id == 0 || !(1..=300_000).contains(&options.validity_ms) || options.timeout_ms == 0 {
        bail!("nonzero ID/timeout and validity 1..=300000 required");
    }
    let timeout = Duration::from_millis(options.timeout_ms);
    let periodic = options
        .period_ms
        .map(|interval| {
            periodic::Setup::new(interval, options.periodic_validity_ms, id, options.mode)
        })
        .transpose()?;
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow!("timeout not representable"))?;
    if options.output.exists() {
        bail!("output directory already exists; preserve earlier evidence");
    }
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115_200)?;
    let _lock = acquire_port_lock(&port)?;
    std::fs::create_dir_all(&options.output)?;
    let console =
        SerialConsole::open_passive(&port, baud, Some(&options.output.join("before.log")))?;
    let mut runtime = Runtime {
        console: Some(console),
        port,
        baud,
        output: options.output,
        id,
        mode: options.mode,
        validity_ms: options.validity_ms,
        timeout,
        deadline,
        mark: 0,
        before: Vec::new(),
        evidence: None,
        periodic,
    };
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/observation-sleep.sw.yaml"),
    )?;
    execute_workflow(
        &workflow,
        &mut runtime,
        &json!({"deep":options.mode==Mode::Deep,"periodic":periodic.is_some(),"passed":false}),
    )?;
    logger.info(format!(
        "Medinote sleep fixture passed: {}",
        runtime.output.join("report.json").display()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct ScriptedRuntime {
        calls: Vec<String>,
        providers: Vec<(String, u64)>,
        fail_at: Option<&'static str>,
        reported: Option<bool>,
    }

    impl WorkflowRuntime for ScriptedRuntime {
        fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
            self.calls.push(action.to_owned());
            if let Some(provider) = args["provider"].as_u64() {
                self.providers.push((action.to_owned(), provider));
            }
            if self.fail_at == Some(action) || action == "fail" {
                bail!("scripted {action} failure");
            }
            let count = self.calls.iter().filter(|call| *call == action).count();
            match action {
                "check_disconnected" => context["disconnected"] = json!(count == 2),
                "reconnect_once" => context["connected"] = json!(count == 2),
                "write_report" => self.reported = context["passed"].as_bool(),
                _ => {}
            }
            Ok(())
        }
    }

    #[test]
    fn workflow_waits_for_both_applications_before_sleep_and_reports_arming_failure() -> Result<()>
    {
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/observation-sleep.sw.yaml"),
        )?;
        for fail_at in [None, Some("start_periodic"), Some("await_periodic")] {
            let mut runtime = ScriptedRuntime {
                fail_at,
                ..Default::default()
            };
            let result = execute_workflow(
                &workflow,
                &mut runtime,
                &json!({"deep":false,"periodic":true,"passed":false}),
            );
            assert_eq!(result.is_ok(), fail_at.is_none());
            assert_eq!(runtime.reported, Some(fail_at.is_none()));
            assert_eq!(
                runtime
                    .calls
                    .iter()
                    .filter(|call| *call == "submit")
                    .count(),
                usize::from(fail_at.is_none())
            );
            assert!(!runtime.calls.iter().any(|call| call == "disconnect"));
            if fail_at.is_none() {
                assert_eq!(
                    runtime.providers,
                    [
                        ("start_periodic".into(), 1),
                        ("await_periodic".into(), 1),
                        ("start_periodic".into(), 2),
                        ("await_periodic".into(), 2)
                    ]
                );
                assert!(
                    runtime
                        .calls
                        .iter()
                        .rposition(|call| call == "await_periodic")
                        .unwrap()
                        < runtime
                            .calls
                            .iter()
                            .position(|call| call == "submit")
                            .unwrap()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn workflow_reconnects_only_for_deep_sleep_and_never_retries_the_command() -> Result<()> {
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/observation-sleep.sw.yaml"),
        )?;
        for deep in [false, true] {
            for fail_at in [
                None,
                Some("check_ready"),
                Some("await_transition"),
                Some("await_recovery"),
            ] {
                let mut runtime = ScriptedRuntime {
                    fail_at,
                    ..Default::default()
                };
                let result = execute_workflow(
                    &workflow,
                    &mut runtime,
                    &json!({"deep":deep,"periodic":false,"passed":false}),
                );
                assert_eq!(result.is_ok(), fail_at.is_none());
                assert_eq!(runtime.reported, Some(fail_at.is_none()));
                let count = |action| runtime.calls.iter().filter(|call| *call == action).count();
                assert_eq!(count("submit"), usize::from(fail_at != Some("check_ready")));
                let reconnect =
                    deep && !matches!(fail_at, Some("check_ready" | "await_transition"));
                assert_eq!(count("disconnect"), usize::from(reconnect));
                assert_eq!(count("check_disconnected"), 2 * usize::from(reconnect));
                assert_eq!(count("reconnect_once"), 2 * usize::from(reconnect));
                assert_eq!(count("poll_interval"), 2 * usize::from(reconnect));
                assert_eq!(count("write_report"), 1);
            }
        }
        Ok(())
    }
}
