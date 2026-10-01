//! Temporary thermal capture primitives; phase order, duration and cleanup live in YAML.
use crate::{
    env_utils,
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::SerialConsole,
    workflows::wifi::common::acquire_port_lock,
};
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Capture {
    console: SerialConsole,
    output: PathBuf,
    evidence: File,
    cursor: usize,
    phase: String,
    paused: bool,
    phase_start: Instant,
    samples: Vec<Value>,
    phases: Vec<Value>,
    last_sample: Option<u64>,
    last_uptime: Option<u64>,
    status: Option<bool>,
    cpu: [Option<u64>; 2],
    refreshes: u64,
}
fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.split_whitespace()
        .find_map(|word| word.strip_prefix(name)?.strip_prefix('='))
}
fn number(line: &str, name: &str) -> Option<i64> {
    field(line, name)?
        .trim_start_matches("Some(")
        .trim_end_matches(')')
        .parse()
        .ok()
}
fn pair(line: &str) -> Option<(u64, i64, i64)> {
    if !line.contains("BME688_DELIVER ") || field(line, "health")? != "Ok" {
        return None;
    }
    Some((
        number(line, "last_sample_at_ms")?.try_into().ok()?,
        number(line, "temperature_centidegrees")?,
        number(line, "sht45_temperature_centidegrees")?,
    ))
}
fn stable(samples: &[Value]) -> bool {
    let Some(tail) = samples.get(samples.len().saturating_sub(3)..) else {
        return false;
    };
    if tail.len() < 3 {
        return false;
    }
    let span = tail[2]["uptime_ms"].as_u64().unwrap() - tail[0]["uptime_ms"].as_u64().unwrap();
    let range = |key: &str| {
        let v: Vec<i64> = tail.iter().map(|s| s[key].as_i64().unwrap()).collect();
        v.iter().max().unwrap() - v.iter().min().unwrap()
    };
    span >= 590_000 && range("delta_centi") <= 15 && range("external_centi") <= 30
}
impl Capture {
    fn record(&mut self, line: &str) -> Result<()> {
        writeln!(
            self.evidence,
            "{} phase={} {}",
            Utc::now().to_rfc3339(),
            self.phase,
            line
        )?;
        self.evidence.flush()?;
        Ok(())
    }
    fn poll(&mut self) -> Result<()> {
        self.console.poll_once()?;
        let lines = self.console.read_recent_lines(self.cursor);
        self.cursor = self.console.mark();
        for line in lines {
            let relevant = [
                "BME688",
                "SHT45",
                "I2C_TIMEOUT",
                "I2C_STARTUP_ERROR",
                "DISPLAYPAUSE",
                "METRICS CPU",
                "CPUPROFILE",
                "LVGL_REFRESH",
                "LVGL_REPAINT",
                "BOOT",
                "rst:",
                "panic",
                "Guru",
                "CPU_CLOCK",
                "FIRMWARE_BOOT",
                "UI_STATE",
                "BATTERY_DELIVER",
                "STATE ",
            ]
            .iter()
            .any(|tag| line.contains(tag));
            if relevant {
                self.record(&line)?;
            }
            if line.contains("CPUPROFILE") && number(&line, "errors").is_some_and(|n| n != 0) {
                bail!("CPU profile accounting overflow: {line}");
            }
            if line.contains("rst:")
                || line.contains("BOOT_RESET")
                || line.contains("panicked")
                || line.contains("Guru Meditation")
            {
                bail!("boot/fault observed during attachment or capture: {line}");
            }
            if line.contains("DISPLAYPAUSE paused=") {
                let uptime = number(&line, "uptime_ms")
                    .ok_or_else(|| anyhow!("malformed pause status"))?
                    as u64;
                if self.last_uptime.is_some_and(|previous| uptime < previous) || uptime < 60_000 {
                    bail!("uptime did not survive attachment: {uptime}");
                }
                self.last_uptime = Some(uptime);
                self.status = Some(field(&line, "paused") == Some("true"));
            }
            if line.contains("METRICS CPU core=") {
                if let (Some(core), Some(percent)) =
                    (number(&line, "core"), number(&line, "percent"))
                {
                    if let Some(slot) = self.cpu.get_mut(core as usize) {
                        *slot = Some(percent as u64);
                    }
                }
            }
            if line.contains("LVGL_REFRESH phase=service") || line.contains("LVGL_REPAINT ") {
                self.refreshes += 1;
                if self.paused && self.phase_start.elapsed() > Duration::from_secs(60) {
                    bail!("display refreshed during B: {line}");
                }
            }
            if let Some((uptime, t, e)) = pair(&line) {
                if self.last_sample == Some(uptime) {
                    continue;
                }
                if self.last_sample.is_some_and(|last| uptime < last) {
                    bail!("sensor uptime regressed");
                }
                self.last_sample = Some(uptime);
                let sample = json!({"utc":Utc::now().to_rfc3339(),"phase":self.phase,"uptime_ms":uptime,"internal_centi":t,"external_centi":e,"delta_centi":t-e,"residual_centi":t-e-434,"cpu":self.cpu});
                self.samples.push(sample.clone());
                let mut f = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.output.join("samples.jsonl"))?;
                writeln!(f, "{sample}")?;
                println!(
                    "{} raw_internal={:.2} external={:.2} delta={:.2} cpu={:?}",
                    self.phase,
                    t as f64 / 100.,
                    e as f64 / 100.,
                    (t - e) as f64 / 100.,
                    self.cpu
                );
            }
        }
        Ok(())
    }
    fn command_status(&mut self, command: &str, expected: Option<bool>) -> Result<()> {
        self.status = None;
        self.record(&format!("HOST_COMMAND {command}"))?;
        self.console.send_line(command)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            self.poll()?;
            if let Some(value) = self.status {
                if expected.is_some_and(|e| e != value) {
                    bail!("unexpected pause status");
                }
                return Ok(());
            }
        }
        bail!("no DISPLAYPAUSE status acknowledgement");
    }
    fn ledger(&self, state: &str) -> Result<()> {
        let tmp = self.output.join("progress.tmp");
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(
                &json!({"utc":Utc::now().to_rfc3339(),"state":state,"phase":self.phase,"elapsed_s":self.phase_start.elapsed().as_secs(),"paused":self.paused,"last_uptime_ms":self.last_uptime,"cpu":self.cpu,"refreshes":self.refreshes,"sample_count":self.samples.len(),"last_sample":self.samples.last(),"stable":stable(&self.samples),"completed_phases":self.phases}),
            )?,
        )?;
        fs::rename(tmp, self.output.join("progress.json"))?;
        Ok(())
    }

    fn action_set_scheduler(&mut self, args: &Value) -> Result<()> {
        let profile = args["profile"]
            .as_str()
            .ok_or_else(|| anyhow!("profile missing"))?;
        if !["AUTO", "INTERACTIVE", "UPLOAD", "DIAGNOSTICS"].contains(&profile) {
            bail!("invalid profile");
        }
        let ack = regex::Regex::new(r"^SCHEDPROFILE ")?;
        if self
            .console
            .command_wait_regex(
                &format!("SCHEDPROFILE {profile}"),
                &ack,
                Duration::from_secs(2),
            )?
            .is_none()
        {
            bail!("scheduler profile acknowledgement missed deadline");
        }
        Ok(())
    }

    fn action_acquisition_window(&mut self) -> Result<()> {
        let ack = regex::Regex::new(r"^ACQWINDOW OK active_ms=30000$")?;
        if self
            .console
            .command_wait_regex("ACQWINDOW", &ack, Duration::from_secs(2))?
            .is_none()
        {
            bail!("acquisition window was not acknowledged");
        }
        Ok(())
    }

    fn qualify_kind(
        &mut self,
        lines: &[String],
        kind: &str,
        args: &Value,
        observations: &mut Vec<Value>,
    ) -> Result<Option<String>> {
        let prefix = format!("METRICS ACQUISITION kind={kind} ");
        let line = lines
            .iter()
            .rev()
            .find(|line| line.starts_with(&prefix))
            .ok_or_else(|| anyhow!("missing {kind} counters"))?;
        let samples = number(line, "samples").ok_or_else(|| anyhow!("missing sample count"))?;
        let max_ms = number(line, "max_ms").ok_or_else(|| anyhow!("missing maximum gap"))?;
        let excluded = number(line, "excluded").ok_or_else(|| anyhow!("missing excluded count"))?;
        observations
            .push(json!({"kind":kind,"samples":samples,"max_ms":max_ms,"excluded":excluded}));
        self.record(line)?;
        if (kind == "imu" && samples < 100)
            || (args["require_touch"].as_bool() == Some(true) && kind != "imu" && samples == 0)
            || (kind == "touch"
                && args["min_touch_samples"]
                    .as_u64()
                    .is_some_and(|minimum| samples < 0 || (samples as u64) < minimum))
        {
            return Ok(Some(format!("{kind} qualification failed: {line}")));
        }
        Ok(None)
    }

    fn record_bus_queues(&mut self, lines: &[String]) -> Result<()> {
        let line = lines
            .iter()
            .rev()
            .find(|line| line.starts_with("METRICS I2C_SERVICE "))
            .ok_or_else(|| anyhow!("missing bus queue counters"))?;
        self.record(line)?;
        for key in ["local_queue_max_us", "remote_queue_max_us"] {
            number(line, key).ok_or_else(|| anyhow!("missing {key}"))?;
        }
        Ok(())
    }

    fn action_check_scheduling(&mut self, args: &Value, context: &mut Value) -> Result<()> {
        let start = context["scheduling_snapshot_start"]
            .as_u64()
            .ok_or_else(|| anyhow!("missing fresh scheduling snapshot"))?
            as usize;
        let lines = self.console.read_recent_lines(start);
        let mut observations = Vec::new();
        let mut failures = Vec::new();
        for kind in ["imu", "touch", "touch_delivery"] {
            if let Some(failure) = self.qualify_kind(&lines, kind, args, &mut observations)? {
                failures.push(failure);
            }
        }
        self.record_bus_queues(&lines)?;
        context[format!("{}_acquisition", self.phase)] = json!(observations);
        if args["defer_qualification_failure"].as_bool() == Some(true) {
            if context["qualification_failures"].is_null() {
                context["qualification_failures"] = json!([]);
            }
            let recorded = context["qualification_failures"]
                .as_array_mut()
                .ok_or_else(|| anyhow!("qualification failure ledger is not an array"))?;
            for failure in failures {
                self.record(&format!("DEFERRED_QUALIFICATION_FAILURE {failure}"))?;
                recorded.push(json!({"phase":self.phase,"reason":failure}));
            }
        } else if let Some(failure) = failures.into_iter().next() {
            bail!("{failure}");
        }
        Ok(())
    }

    fn action_probe_metrics_control(&mut self, context: &mut Value) -> Result<()> {
        let mark = self.console.mark();
        self.console.send_line("METRICS")?;
        let begun = regex::Regex::new(r"^METRICS ")?;
        if self
            .console
            .wait_for_regex_since(mark, &begun, Duration::from_secs(2))?
            .is_none()
        {
            bail!("metrics response did not begin");
        }
        let ping_mark = self.console.mark();
        let started = Instant::now();
        self.console.send_line("PING")?;
        let pong = regex::Regex::new(r"^PONG(?: |$)")?;
        if self
            .console
            .wait_for_regex_since(ping_mark, &pong, Duration::from_secs(2))?
            .is_none()
        {
            bail!("PING missed the 2-second control deadline during metrics");
        }
        context["metrics_ping_ms"] = json!(started.elapsed().as_millis());
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            self.poll()?;
        }
        let lines = self.console.read_recent_lines(ping_mark);
        let pong_at = lines
            .iter()
            .position(|line| pong.is_match(line))
            .ok_or_else(|| anyhow!("PONG evidence missing"))?;
        let continued = lines[pong_at + 1..]
            .iter()
            .any(|line| line.starts_with("METRICS ") || line.starts_with("CPUPROFILE "));
        context["metrics_continued_after_pong"] = json!(continued);
        if !continued {
            bail!("metrics ended before PONG; independent control progress not demonstrated");
        }
        let end = regex::Regex::new(r"^METRICS NET_ACCEPT ")?;
        if self
            .console
            .wait_for_regex_since(mark, &end, Duration::from_secs(10))?
            .is_none()
        {
            bail!("control probe metrics did not complete");
        }
        self.record(&format!(
            "SERIAL_PROGRESS ping_ms={} metrics_continued=true",
            context["metrics_ping_ms"]
        ))?;
        Ok(())
    }

    fn action_await_operator(&mut self) -> Result<()> {
        fs::write(
            self.output.join("READY"),
            b"connection settled; create START to begin
",
        )?;
        self.ledger("awaiting_input")?;
        let deadline = Instant::now() + Duration::from_secs(600);
        while !self.output.join("START").exists() {
            if self.output.join("STOP").exists() || Instant::now() >= deadline {
                bail!("physical check cancelled or operator start expired");
            }
            self.poll()?;
        }
        self.record("PHYSICAL_INPUT_STARTED")?;
        Ok(())
    }

    fn action_settle_attachment(&mut self, args: &Value) -> Result<()> {
        let seconds = args["seconds"]
            .as_u64()
            .ok_or_else(|| anyhow!("settle duration missing"))?;
        let deadline = Instant::now() + Duration::from_secs(seconds);
        // Serial attachment may reset this adapter/board. Retain raw boot
        // evidence, then start fault/uptime checks after the bounded settle.
        while Instant::now() < deadline {
            self.console.poll_once()?;
        }
        self.cursor = self.console.mark();
        self.record("ATTACHMENT_SETTLED")?;
        Ok(())
    }

    fn action_verify_attach(&mut self) -> Result<()> {
        self.command_status("DISPLAYPAUSE", None)?;
        if self.status == Some(true) {
            bail!("already paused before experiment");
        }
        self.ledger("attached")?;
        Ok(())
    }

    fn action_reset_scheduling_metrics(&mut self) -> Result<()> {
        let ack = regex::Regex::new(r"^TOUCHSCHEDRESET OK$")?;
        if self
            .console
            .command_wait_regex("TOUCHSCHEDRESET", &ack, Duration::from_secs(3))?
            .is_none()
        {
            bail!("scheduling metrics reset was not acknowledged");
        }
        Ok(())
    }

    fn action_begin_phase(&mut self, args: &Value) -> Result<()> {
        self.phase = args["name"]
            .as_str()
            .ok_or_else(|| anyhow!("phase missing"))?
            .to_owned();
        self.paused = args["paused"]
            .as_bool()
            .ok_or_else(|| anyhow!("pause missing"))?;
        self.samples.clear();
        self.refreshes = 0;
        self.phase_start = Instant::now();
        self.command_status(
            if self.paused {
                "DISPLAYPAUSE ON"
            } else {
                "DISPLAYPAUSE OFF"
            },
            Some(self.paused),
        )?;
        self.ledger("running")?;
        println!("phase={} started paused={}", self.phase, self.paused);
        Ok(())
    }

    fn action_capture_window(&mut self, args: &Value, context: &mut Value) -> Result<()> {
        let seconds = args["seconds"]
            .as_u64()
            .ok_or_else(|| anyhow!("window missing"))?;
        let deadline = Instant::now() + Duration::from_secs(seconds);
        self.command_status("DISPLAYPAUSE", Some(self.paused))?;
        let start = self.console.mark();
        let end = regex::Regex::new(r"^METRICS NET_ACCEPT ")?;
        if self
            .console
            .command_wait_inkplate_metrics(&end, Duration::from_secs(10))?
            .is_none()
        {
            bail!("fresh metrics snapshot did not complete");
        }
        context["scheduling_snapshot_start"] = json!(start);
        self.poll()?;
        while Instant::now() < deadline {
            if self.output.join("STOP").exists() {
                bail!("operator stop requested");
            }
            self.poll()?;
        }
        context["phase_elapsed_s"] = json!(self.phase_start.elapsed().as_secs());
        context["phase_stable"] = json!(stable(&self.samples));
        if let Some(last) = self.samples.last() {
            let sample = last["uptime_ms"].as_u64().unwrap();
            if self
                .last_uptime
                .is_some_and(|now| now.saturating_sub(sample) > 420_000)
            {
                bail!("environmental sampling stalled");
            }
        } else if self.phase_start.elapsed() > Duration::from_secs(420) {
            bail!("no paired sample in phase");
        }
        self.ledger("running")?;
        Ok(())
    }

    fn action_finish_phase(&mut self) -> Result<()> {
        self.phases.push(json!({"phase":self.phase,"seconds":self.phase_start.elapsed().as_secs(),"stable":stable(&self.samples),"refreshes":self.refreshes,"samples":self.samples}));
        self.ledger("running")?;
        Ok(())
    }

    fn action_restore(&mut self, context: &mut Value) -> Result<()> {
        self.command_status("DISPLAYPAUSE OFF", Some(false))?;
        self.paused = false;
        context["restored"] = json!(true);
        Ok(())
    }

    fn action_capture_reset_trace(&mut self, args: &Value, context: &mut Value) -> Result<()> {
        let seconds = args["seconds"]
            .as_u64()
            .ok_or_else(|| anyhow!("reset capture duration missing"))?;
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let mut cursor = self.console.mark();
        let mut boot_seen = false;
        println!("retaining serial connection for up to {seconds}s after failure");
        loop {
            for line in self.console.read_recent_lines(cursor) {
                if line.starts_with("BOOT_RESET reason=") {
                    boot_seen = true;
                } else if boot_seen && line.starts_with("STALL_RECORD ") {
                    context["reset_capture_complete"] = json!(true);
                    return Ok(());
                }
            }
            cursor = self.console.mark();
            if Instant::now() >= deadline {
                context["reset_capture_complete"] = json!(false);
                return Ok(());
            }
            if let Err(error) = self.console.poll_once() {
                context["reset_capture_error"] = json!(error.to_string());
                return Ok(());
            }
        }
    }

    fn action_finish(&mut self, context: &mut Value) -> Result<()> {
        let passed = context.get("experiment_error").is_none()
            && context.get("restore_error").is_none()
            && !qualification_failed(context);
        fs::write(
            self.output.join("result.json"),
            serde_json::to_vec_pretty(
                &json!({"completed":passed,"context":context,"phases":self.phases}),
            )?,
        )?;
        self.ledger(if passed { "complete" } else { "failed" })?;
        if !passed {
            bail!("experiment failed; inspect result.json");
        }
        Ok(())
    }
}
fn qualification_failed(context: &Value) -> bool {
    context["qualification_failures"]
        .as_array()
        .is_some_and(|failures| !failures.is_empty())
}
impl WorkflowRuntime for Capture {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "set_scheduler" => self.action_set_scheduler(args),
            "acquisition_window" => self.action_acquisition_window(),
            "check_scheduling" => self.action_check_scheduling(args, context),
            "probe_metrics_control" => self.action_probe_metrics_control(context),
            "await_operator" => self.action_await_operator(),
            "settle_attachment" => self.action_settle_attachment(args),
            "verify_attach" => self.action_verify_attach(),
            "reset_scheduling_metrics" => self.action_reset_scheduling_metrics(),
            "begin_phase" => self.action_begin_phase(args),
            "capture_window" => self.action_capture_window(args, context),
            "capture_reset_trace" => self.action_capture_reset_trace(args, context),
            "finish_phase" => self.action_finish_phase(),
            "restore" => self.action_restore(context),
            "finish" => self.action_finish(context),
            _ => bail!("unknown thermal action {action}"),
        }
    }
}
pub fn run(output: PathBuf) -> Result<()> {
    run_scenario(output, "thermal-aba.sw.yaml")
}
pub fn run_multicore(
    output: PathBuf,
    physical_input: bool,
    short_screen: bool,
    upload_short: bool,
) -> Result<()> {
    run_scenario(
        output,
        if upload_short {
            "i2c-fifo-upload-short.sw.yaml"
        } else if short_screen {
            "multicore-physical-short-screen.sw.yaml"
        } else if physical_input {
            "multicore-physical-qualification.sw.yaml"
        } else {
            "multicore-qualification.sw.yaml"
        },
    )
}
pub fn run_profile(output: PathBuf, active_only: bool, probe_control: bool) -> Result<()> {
    run_scenario(
        output,
        if probe_control {
            "cpu-profile-control.sw.yaml"
        } else if active_only {
            "cpu-profile-active.sw.yaml"
        } else {
            "cpu-profile.sw.yaml"
        },
    )
}
fn run_scenario(output: PathBuf, scenario: &str) -> Result<()> {
    if output.exists() {
        bail!("output already exists; retain earlier evidence");
    }
    let port = env_utils::require_port()?;
    let _lock = acquire_port_lock(&port)?;
    fs::create_dir_all(&output)?;
    let console = SerialConsole::open_passive(&port, 115200, Some(&output.join("serial.log")))?;
    let evidence = File::create(output.join("evidence.log"))?;
    let mut capture = Capture {
        console,
        output,
        evidence,
        cursor: 0,
        phase: "attach".into(),
        paused: false,
        phase_start: Instant::now(),
        samples: vec![],
        phases: vec![],
        last_sample: None,
        last_uptime: None,
        status: None,
        cpu: [None, None],
        refreshes: 0,
    };
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scenarios")
            .join(scenario),
    )?;
    execute_workflow(&workflow, &mut capture, &json!({}))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial_console::mock::MockPort;

    fn test_capture() -> Result<(Capture, tempfile::TempDir)> {
        let directory = tempfile::tempdir()?;
        let capture = Capture {
            console: SerialConsole::from_port_for_tests(Box::new(MockPort::new()), None)?,
            output: directory.path().to_path_buf(),
            evidence: File::create(directory.path().join("evidence.log"))?,
            cursor: 0,
            phase: "interactive".into(),
            paused: false,
            phase_start: Instant::now(),
            samples: vec![],
            phases: vec![],
            last_sample: None,
            last_uptime: None,
            status: None,
            cpu: [None, None],
            refreshes: 0,
        };
        Ok((capture, directory))
    }

    #[test]
    fn scheduling_records_long_gaps_and_queues_without_failing_coverage() -> Result<()> {
        let (mut capture, _directory) = test_capture()?;
        let lines = [
            "METRICS ACQUISITION kind=touch samples=216 max_ms=18 excluded=206",
            "METRICS ACQUISITION kind=imu samples=5004 max_ms=17 excluded=45",
            "METRICS ACQUISITION kind=touch_delivery samples=612 max_ms=21 excluded=0",
            "METRICS I2C_SERVICE local_queue_max_us=6555 remote_queue_max_us=2118",
        ]
        .map(str::to_owned);
        let mut observations = Vec::new();
        for kind in ["imu", "touch", "touch_delivery"] {
            assert!(capture
                .qualify_kind(
                    &lines,
                    kind,
                    &json!({"require_touch":true}),
                    &mut observations
                )?
                .is_none());
        }
        capture.record_bus_queues(&lines)?;
        assert_eq!(observations.len(), 3);
        assert_eq!(observations[1]["max_ms"], 18);

        let missing_touch = ["METRICS ACQUISITION kind=touch samples=0 max_ms=0 excluded=0".into()];
        assert!(capture
            .qualify_kind(
                &missing_touch,
                "touch",
                &json!({"require_touch":true}),
                &mut observations,
            )?
            .is_some());
        Ok(())
    }

    #[test]
    fn fifo_upload_screen_is_one_operator_window_and_restores_auto() -> Result<()> {
        #[derive(Default)]
        struct Probe {
            armed: bool,
            profiles: Vec<String>,
            windows: Vec<u64>,
            checks: usize,
        }
        impl WorkflowRuntime for Probe {
            fn invoke(&mut self, action: &str, args: &Value, _: &mut Value) -> Result<()> {
                match action {
                    "await_operator" => self.armed = true,
                    "set_scheduler" => {
                        assert!(self.armed);
                        self.profiles.push(args["profile"].as_str().unwrap().into());
                    }
                    "capture_window" => self.windows.push(args["seconds"].as_u64().unwrap()),
                    "check_scheduling" => {
                        assert_eq!(args["require_touch"], true);
                        self.checks += 1;
                    }
                    _ => {}
                }
                Ok(())
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("scenarios/i2c-fifo-upload-short.sw.yaml"),
        )?;
        let mut probe = Probe::default();
        execute_workflow(&workflow, &mut probe, &json!({}))?;
        assert_eq!(probe.profiles, ["UPLOAD", "AUTO"]);
        assert_eq!(probe.windows, [34, 2]);
        assert_eq!(probe.checks, 1);
        Ok(())
    }
    #[test]
    fn multicore_workflow_restores_auto_and_stops_after_a_failed_check() -> Result<()> {
        struct Probe {
            fail: bool,
            profiles: Vec<String>,
            checks: usize,
        }
        impl WorkflowRuntime for Probe {
            fn invoke(&mut self, action: &str, args: &Value, _: &mut Value) -> Result<()> {
                if action == "set_scheduler" {
                    self.profiles.push(args["profile"].as_str().unwrap().into());
                }
                if action == "check_scheduling" {
                    self.checks += 1;
                    if self.fail {
                        bail!("latency violation");
                    }
                }
                Ok(())
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("scenarios/multicore-qualification.sw.yaml"),
        )?;
        for fail in [false, true] {
            let mut probe = Probe {
                fail,
                profiles: vec![],
                checks: 0,
            };
            let context = execute_workflow(&workflow, &mut probe, &json!({}))?;
            assert_eq!(context.get("experiment_error").is_some(), fail);
            assert_eq!(probe.profiles.last().map(String::as_str), Some("AUTO"));
            assert_eq!(probe.checks, if fail { 1 } else { 3 });
            if !fail {
                assert_eq!(
                    probe.profiles,
                    ["INTERACTIVE", "UPLOAD", "DIAGNOSTICS", "AUTO"]
                );
            }
        }
        Ok(())
    }

    #[test]
    fn physical_workflow_waits_for_operator_and_requires_each_profile_coverage() -> Result<()> {
        #[derive(Default)]
        struct Probe {
            armed: bool,
            fail: bool,
            coverage_failure: bool,
            windows: usize,
            checks: usize,
            reset_captures: usize,
        }
        impl WorkflowRuntime for Probe {
            fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
                match action {
                    "await_operator" => self.armed = true,
                    "set_scheduler" => assert!(self.armed),
                    "capture_window" if args["seconds"] == 60 => {
                        self.windows += 1;
                        if self.fail {
                            bail!("simulated physical input stall");
                        }
                    }
                    "capture_reset_trace" => self.reset_captures += 1,
                    "check_scheduling" => {
                        assert_eq!(args["require_touch"], true);
                        assert_eq!(args["defer_qualification_failure"], true);
                        self.checks += 1;
                        if self.coverage_failure && self.checks == 1 {
                            context["qualification_failures"] =
                                json!([{"phase":"interactive","reason":"missing imu coverage"}]);
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("scenarios/multicore-physical-qualification.sw.yaml"),
        )?;
        let mut probe = Probe::default();
        execute_workflow(&workflow, &mut probe, &json!({}))?;
        assert_eq!((probe.windows, probe.checks), (3, 3));
        assert_eq!(probe.reset_captures, 0);
        let mut failed = Probe {
            fail: true,
            ..Probe::default()
        };
        let context = execute_workflow(&workflow, &mut failed, &json!({}))?;
        assert!(context.get("experiment_error").is_some());
        assert_eq!((failed.windows, failed.reset_captures), (1, 1));
        let mut coverage_failed = Probe {
            coverage_failure: true,
            ..Probe::default()
        };
        let context = execute_workflow(&workflow, &mut coverage_failed, &json!({}))?;
        assert!(context.get("experiment_error").is_none());
        assert!(qualification_failed(&context));
        assert_eq!((coverage_failed.windows, coverage_failed.checks), (3, 3));
        assert_eq!(coverage_failed.reset_captures, 0);
        Ok(())
    }

    #[test]
    fn workflow_runs_all_phases_and_restores_after_failure() -> Result<()> {
        struct Probe {
            fail: bool,
            calls: Vec<String>,
        }
        impl WorkflowRuntime for Probe {
            fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
                self.calls.push(format!(
                    "{}:{}",
                    action,
                    args["name"].as_str().unwrap_or("")
                ));
                if action == "capture_window" {
                    if self.fail {
                        bail!("simulated capture failure");
                    }
                    context["phase_elapsed_s"] = json!(1500);
                    context["phase_stable"] = json!(true);
                }
                Ok(())
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/thermal-aba.sw.yaml"),
        )?;
        let mut probe = Probe {
            fail: false,
            calls: vec![],
        };
        execute_workflow(&workflow, &mut probe, &json!({}))?;
        assert!(probe.calls.contains(&"begin_phase:A1".to_owned()));
        assert!(probe.calls.contains(&"begin_phase:B".to_owned()));
        assert!(probe.calls.contains(&"begin_phase:A2".to_owned()));
        assert_eq!(
            &probe.calls[probe.calls.len() - 2..],
            &["restore:", "finish:"]
        );
        let mut probe = Probe {
            fail: true,
            calls: vec![],
        };
        let context = execute_workflow(&workflow, &mut probe, &json!({}))?;
        assert!(context.get("experiment_error").is_some());
        assert!(!probe.calls.contains(&"begin_phase:A1".to_owned()));
        assert_eq!(
            &probe.calls[probe.calls.len() - 2..],
            &["restore:", "finish:"]
        );
        Ok(())
    }
    #[test]
    fn paired_values_require_external_and_health_and_accept_optional_wire_values() {
        assert_eq!(pair("BME688_DELIVER temperature_centidegrees=3749 sht45_temperature_centidegrees=Some(2756) health=Ok last_sample_at_ms=Some(4004)"),Some((4004,3749,2756)));
        assert!(pair("BME688_DELIVER temperature_centidegrees=3749 sht45_temperature_centidegrees=None health=Ok last_sample_at_ms=Some(4004)").is_none());
        assert!(pair("BME688_DELIVER temperature_centidegrees=3749 sht45_temperature_centidegrees=2756 health=Failed last_sample_at_ms=4004").is_none());
    }
    #[test]
    fn stability_requires_ten_minutes_and_bounded_reference_and_delta() {
        let s = |u, d, e| json!({"uptime_ms":u,"delta_centi":d,"external_centi":e});
        assert!(stable(&[
            s(0, 900, 2700),
            s(300000, 908, 2705),
            s(600000, 910, 2710)
        ]));
        assert!(!stable(&[
            s(0, 900, 2700),
            s(300000, 908, 2705),
            s(600000, 920, 2710)
        ]));
        assert!(!stable(&[
            s(0, 900, 2700),
            s(1000, 908, 2705),
            s(2000, 910, 2710)
        ]));
        assert!(!stable(&[
            s(0, 900, 2700),
            s(300000, 908, 2705),
            s(600000, 910, 2740)
        ]));
    }
}
