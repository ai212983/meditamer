//! Bounded firmware trace capture primitives; ordering and cleanup live in YAML.

use crate::{
    env_utils,
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::SerialConsole,
    workflows::wifi::common::acquire_port_lock,
};
use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const SELF_TEST_TIMEOUT: Duration = Duration::from_secs(10);
const DUMP_TIMEOUT: Duration = Duration::from_secs(30);

struct TraceCapture {
    console: SerialConsole,
    output: PathBuf,
    seconds: Option<u64>,
}

fn command_ack(console: &mut SerialConsole, command: &str, expected: &Regex) -> Result<String> {
    let mark = console.mark();
    console.send_line(command)?;
    let response = Regex::new(&format!(r"^(?:{}|TRACE ERROR(?: .*)?)$", expected.as_str()))?;
    let line = console
        .wait_for_regex_since(mark, &response, COMMAND_TIMEOUT)?
        .ok_or_else(|| anyhow!("{command} response timed out"))?;
    if line.starts_with("TRACE ERROR") {
        bail!("device rejected {command}: {line}");
    }
    Ok(line)
}

#[derive(Default)]
struct DumpAccumulator {
    generation: Option<u64>,
    expected_count: Option<usize>,
    events: usize,
    first_sequence: Option<usize>,
    ended: bool,
    incomplete: bool,
}

fn header_fields(line: &str) -> Result<std::collections::HashMap<&str, &str>> {
    let mut values = std::collections::HashMap::new();
    for token in line["TRACE BEGIN ".len()..].split_whitespace() {
        let pair = token
            .split_once('=')
            .expect("header regex requires key=value");
        if values.insert(pair.0, pair.1).is_some() {
            bail!("duplicate trace header field {}", pair.0);
        }
    }
    Ok(values)
}

fn check_event_floats(line: &str) -> Result<()> {
    for token in line
        .split_whitespace()
        .filter(|token| token.starts_with("f."))
    {
        let (_, typed) = token
            .split_once('=')
            .expect("event regex requires key=value");
        if let Some(raw) = typed.strip_prefix("f:") {
            let value = raw.parse::<f64>()?;
            if !value.is_finite() {
                bail!("trace event contains a non-finite float");
            }
        }
    }
    Ok(())
}

impl DumpAccumulator {
    fn ingest_header(&mut self, line: &str) -> Result<()> {
        if self.generation.is_some() {
            bail!("multiple TRACE BEGIN records in dump");
        }
        let values = header_fields(line)?;
        for required in [
            "version",
            "generation",
            "count",
            "overflow",
            "unsupported",
            "hook_failures",
        ] {
            if !values.contains_key(required) {
                bail!("trace header missing {required}");
            }
        }
        if values["version"] != "1" {
            bail!("unsupported trace version {}", values["version"]);
        }
        self.generation = Some(values["generation"].parse::<u64>()?);
        self.expected_count = Some(values["count"].parse::<usize>()?);
        self.incomplete |= values.iter().any(|(key, value)| {
            !matches!(*key, "version" | "generation" | "count") && *value != "0"
        });
        Ok(())
    }

    fn ingest_event(&mut self, line: &str, captures: &regex::Captures<'_>) -> Result<()> {
        if self.generation.is_none() || self.ended {
            bail!("TRACE EVENT outside dump boundaries: {line}");
        }
        let sequence = captures[1].parse::<usize>()?;
        let expected = self
            .first_sequence
            .get_or_insert(sequence)
            .saturating_add(self.events);
        if sequence != expected {
            bail!("trace event sequence gap, duplicate, or reordering");
        }
        check_event_floats(line)?;
        self.events += 1;
        Ok(())
    }

    fn ingest_end(&mut self, captures: &regex::Captures<'_>) -> Result<()> {
        let end_generation = captures[1].parse::<u64>()?;
        if self.generation != Some(end_generation) || self.ended {
            bail!("TRACE END generation mismatch or duplicate");
        }
        self.ended = true;
        Ok(())
    }

    fn ingest_error(&mut self, line: &str) -> Result<()> {
        if self.generation.is_none() || self.ended {
            bail!("firmware reported {line} outside dump boundaries");
        }
        self.incomplete = true;
        Ok(())
    }

    fn finish(self) -> Result<(u64, usize, bool)> {
        let generation = self
            .generation
            .ok_or_else(|| anyhow!("dump omitted TRACE BEGIN"))?;
        if !self.ended {
            bail!("dump omitted TRACE END");
        }
        let events = self.events;
        if self.expected_count != Some(events) {
            bail!(
                "trace header count {} does not match {events} events",
                self.expected_count.unwrap_or_default()
            );
        }
        Ok((generation, events, self.incomplete))
    }
}

fn parse_dump(lines: &[String]) -> Result<(u64, usize, bool)> {
    let header = Regex::new(r"^TRACE BEGIN(?: [a-z_]+=[0-9]+)+$")?;
    let event = Regex::new(
        r"^TRACE EVENT seq=([0-9]+) ts=[0-9]+ core=[0-9]+ context_kind=[^ ]+ context_id=[0-9]+ target=[^ ]+ name=[^ ]+(?: f\.[^ =]+=(?:u:[0-9]+|i:-?[0-9]+|b:[01]|f:[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?))*$",
    )?;
    let end = Regex::new(r"^TRACE END generation=([0-9]+)$")?;
    let mut dump = DumpAccumulator::default();
    for line in lines {
        if header.is_match(line) {
            dump.ingest_header(line)?;
        } else if let Some(captures) = event.captures(line) {
            dump.ingest_event(line, &captures)?;
        } else if let Some(captures) = end.captures(line) {
            dump.ingest_end(&captures)?;
        } else if line.starts_with("TRACE ERROR") {
            dump.ingest_error(line)?;
        } else {
            bail!("malformed trace record: {line}");
        }
    }
    dump.finish()
}

fn unsigned_field(fields: &std::collections::HashMap<&str, &str>, name: &str) -> Result<u64> {
    let raw = fields
        .get(name)
        .ok_or_else(|| anyhow!("probe event missing {name}"))?;
    raw.strip_prefix("u:")
        .ok_or_else(|| anyhow!("probe field {name} is not u64"))?
        .parse::<u64>()
        .with_context(|| format!("invalid probe field {name}"))
}

fn verify_probe_lines(lines: &[String]) -> Result<()> {
    let (_, _, incomplete) = parse_dump(lines)?;
    if incomplete {
        bail!("self-test dump reports nonzero loss counters or formatting errors");
    }
    let events: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with("TRACE EVENT "))
        .collect();
    let first = events
        .first()
        .ok_or_else(|| anyhow!("self-test dump contains no events"))?;
    let first_fields: std::collections::HashMap<_, _> = first
        .split_whitespace()
        .filter_map(|token| token.split_once('='))
        .collect();
    if first_fields.get("target") != Some(&"firmware")
        || first_fields.get("name") != Some(&"capture_started")
    {
        bail!("first self-test event is not firmware capture_started");
    }

    let mut seen = [[false; 16]; 2];
    for line in events {
        let fields: std::collections::HashMap<_, _> = line
            .split_whitespace()
            .filter_map(|token| token.split_once('='))
            .collect();
        if fields.get("target") != Some(&"firmware") || fields.get("name") != Some(&"probe") {
            continue;
        }
        let producer = unsigned_field(&fields, "f.producer")?;
        let index = unsigned_field(&fields, "f.index")?;
        let value = unsigned_field(&fields, "f.value")?;
        let inverse = unsigned_field(&fields, "f.inverse")?;
        if producer > 1 || index >= 16 {
            bail!("probe producer/index out of range: producer={producer} index={index}");
        }
        let core = fields
            .get("core")
            .ok_or_else(|| anyhow!("probe event missing core"))?
            .parse::<u64>()?;
        if core != producer {
            bail!("probe core {core} does not match producer {producer}");
        }
        let expected = (producer << 32) | index;
        if value != expected || inverse != !expected {
            bail!("probe pattern mismatch for producer={producer} index={index}");
        }
        let slot = &mut seen[producer as usize][index as usize];
        if *slot {
            bail!("duplicate probe producer={producer} index={index}");
        }
        *slot = true;
    }
    for (producer, row) in seen.iter().enumerate() {
        for (index, present) in row.iter().enumerate() {
            if !present {
                bail!("missing probe producer={producer} index={index}");
            }
        }
    }
    Ok(())
}

impl TraceCapture {
    fn settle_attachment(&mut self, args: &Value) -> Result<()> {
        let seconds = args["seconds"]
            .as_u64()
            .ok_or_else(|| anyhow!("settle duration missing"))?;
        self.console.settle(seconds.saturating_mul(1_000))?;
        Ok(())
    }

    fn verify_ready(&mut self) -> Result<()> {
        let mark = self.console.mark();
        self.console.send_line("PING")?;
        let response = Regex::new(r"^(?:PONG(?: .*)?|TRACE ERROR(?: .*)?)$")?;
        let line = self
            .console
            .wait_for_regex_since(mark, &response, COMMAND_TIMEOUT)?
            .ok_or_else(|| anyhow!("device did not become ready after attachment settle"))?;
        if line.starts_with("TRACE ERROR") {
            bail!("device readiness failed: {line}");
        }
        Ok(())
    }

    fn arm_trace(&mut self, context: &mut Value) -> Result<()> {
        let ack = Regex::new(r"TRACE ARM OK generation=[0-9]+ capacity=[0-9]+")?;
        // Once the bytes leave the host, cleanup must assume ARM may
        // have succeeded even if its acknowledgement is lost.
        context["trace_started"] = json!(true);
        command_ack(&mut self.console, "TRACE ARM", &ack)?;
        context["trace_state"] = json!("armed");
        Ok(())
    }

    fn run_self_test(&mut self, context: &mut Value) -> Result<()> {
        let mark = self.console.mark();
        context["trace_started"] = json!(true);
        self.console.send_line("TRACE SELFTEST")?;
        let response =
            Regex::new(r"^(?:TRACE SELFTEST OK generation=[0-9]+|TRACE ERROR(?: .*)?)$")?;
        let line = self
            .console
            .wait_for_regex_since(mark, &response, SELF_TEST_TIMEOUT)?
            .ok_or_else(|| anyhow!("TRACE SELFTEST response timed out"))?;
        if line.starts_with("TRACE ERROR") {
            bail!("device rejected TRACE SELFTEST: {line}");
        }
        context["trace_state"] = json!("frozen");
        Ok(())
    }

    fn mark_ready(&mut self) -> Result<()> {
        let message: &[u8] = if self.seconds.is_some() {
            b"trace armed; timed capture starting\n"
        } else {
            b"trace armed; perform the action, then create STOP when done\n"
        };
        fs::write(self.output.join("READY"), message)?;
        Ok(())
    }

    fn poll_status(&mut self, context: &mut Value) -> Result<()> {
        let mark = self.console.mark();
        self.console.send_line("TRACE STATUS")?;
        let response = Regex::new(
            r"^(?:TRACE STATUS state=(armed|capturing|frozen|full) generation=[0-9]+ count=[0-9]+|TRACE ERROR(?: .*)?)$",
        )?;
        let line = self
            .console
            .wait_for_regex_since(mark, &response, COMMAND_TIMEOUT)?
            .ok_or_else(|| anyhow!("TRACE STATUS response timed out"))?;
        if line.starts_with("TRACE ERROR") {
            bail!("device rejected TRACE STATUS: {line}");
        }
        let captures = response
            .captures(&line)
            .ok_or_else(|| anyhow!("malformed TRACE STATUS response"))?;
        context["trace_state"] = json!(&captures[1]);
        Ok(())
    }

    fn wait_poll(&mut self, context: &mut Value) -> Result<()> {
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            self.console.poll_once()?;
        }
        context["stop_requested"] = json!(self.output.join("STOP").exists());
        Ok(())
    }

    fn mark_recording(&mut self, context: &mut Value) -> Result<()> {
        fs::write(self.output.join("RECORDING"), b"first action observed\n")?;
        context["recording_seen"] = json!(true);
        Ok(())
    }

    fn mark_frozen(&mut self, context: &mut Value) -> Result<()> {
        fs::write(
            self.output.join("FROZEN"),
            b"trace buffer reached capacity\n",
        )?;
        context["trace_full"] = json!(true);
        Ok(())
    }

    fn capture_window(&mut self) -> Result<()> {
        let seconds = self
            .seconds
            .ok_or_else(|| anyhow!("timed capture duration missing"))?;
        command_ack(
            &mut self.console,
            "TRACE START",
            &Regex::new(r"TRACE START OK generation=[0-9]+ capacity=[0-9]+")?,
        )?;
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            if self.output.join("STOP").exists() {
                break;
            }
            self.console.poll_once()?;
        }
        Ok(())
    }

    fn stop_trace(&mut self, context: &mut Value) -> Result<()> {
        let ack = Regex::new(r"TRACE STOP OK generation=[0-9]+ count=[0-9]+")?;
        command_ack(&mut self.console, "TRACE STOP", &ack)?;
        context["trace_stopped"] = json!(true);
        Ok(())
    }

    fn dump_trace(&mut self, context: &mut Value) -> Result<()> {
        let mark = self.console.mark();
        self.console.send_line("TRACE DUMP")?;
        let end = Regex::new(r"^TRACE END generation=[0-9]+$")?;
        let error = Regex::new(r"^TRACE ERROR(?: .*)?$")?;
        let deadline = Instant::now() + DUMP_TIMEOUT;
        let mut response_error = None;
        loop {
            self.console.poll_once()?;
            let recent = self.console.read_recent_lines(mark);
            if recent.iter().any(|line| end.is_match(line)) {
                break;
            }
            let began = recent.iter().any(|line| line.starts_with("TRACE BEGIN "));
            if !began && recent.iter().any(|line| error.is_match(line)) {
                response_error = Some(anyhow!("TRACE DUMP was rejected"));
                break;
            }
            if Instant::now() >= deadline {
                response_error = Some(anyhow!("TRACE DUMP response timed out"));
                break;
            }
        }
        let trace_lines: Vec<String> = self
            .console
            .read_recent_lines(mark)
            .into_iter()
            .filter(|line| line.starts_with("TRACE"))
            .collect();
        let trace_path = self.output.join("trace.log");
        fs::write(&trace_path, format!("{}\n", trace_lines.join("\n")))
            .with_context(|| format!("write {}", trace_path.display()))?;
        if let Some(error) = response_error {
            return Err(error);
        }
        let (generation, count, incomplete) = parse_dump(&trace_lines)?;
        context["generation"] = json!(generation);
        context["event_count"] = json!(count);
        context["trace_incomplete"] = json!(incomplete);
        context["trace_dumped"] = json!(true);
        Ok(())
    }

    fn verify_probe(&mut self, context: &mut Value) -> Result<()> {
        let trace_path = self.output.join("trace.log");
        let lines = fs::read_to_string(&trace_path)
            .with_context(|| format!("read {}", trace_path.display()))?
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        verify_probe_lines(&lines)?;
        context["self_test_verified"] = json!(true);
        Ok(())
    }

    fn finish_capture(&mut self, context: &mut Value) -> Result<()> {
        let passed = context.get("capture_error").is_none()
            && context.get("stop_error").is_none()
            && context.get("dump_error").is_none()
            && context.get("verify_error").is_none()
            && context["trace_incomplete"] != true;
        fs::write(
            self.output.join("result.json"),
            serde_json::to_vec_pretty(&json!({
                "completed": passed,
                "context": context,
                "export": "python3 tools/trace_export/trace_export.py trace.log --output trace.json --report trace.md"
            }))?,
        )?;
        if !passed {
            bail!("trace capture failed; raw serial and available dump evidence were preserved");
        }
        println!("trace={}", self.output.join("trace.log").display());
        println!(
            "export: python3 tools/trace_export/trace_export.py {} --output {} --report {}",
            self.output.join("trace.log").display(),
            self.output.join("trace.json").display(),
            self.output.join("trace.md").display()
        );
        Ok(())
    }
}

impl WorkflowRuntime for TraceCapture {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "settle_attachment" => self.settle_attachment(args),
            "verify_ready" => self.verify_ready(),
            "arm_trace" => self.arm_trace(context),
            "self_test" => self.run_self_test(context),
            "mark_ready" => self.mark_ready(),
            "poll_status" => self.poll_status(context),
            "wait_poll" => self.wait_poll(context),
            "mark_recording" => self.mark_recording(context),
            "mark_frozen" => self.mark_frozen(context),
            "capture_window" => self.capture_window(),
            "capture_done" => Ok(()),
            "stop_trace" => self.stop_trace(context),
            "dump_trace" => self.dump_trace(context),
            "verify_probe" => self.verify_probe(context),
            "finish" => self.finish_capture(context),
            _ => bail!("unknown trace capture action {action}"),
        }
    }
}

pub fn run(output: PathBuf, seconds: Option<u64>, self_test: bool) -> Result<()> {
    if output.exists() {
        bail!("output already exists; retain earlier evidence");
    }
    if seconds == Some(0) {
        bail!("capture duration must be at least one second");
    }
    let port = env_utils::require_port()?;
    let _lock = acquire_port_lock(&port)?;
    fs::create_dir_all(&output)?;
    let console = SerialConsole::open_passive(&port, 115_200, Some(&output.join("serial.log")))?;
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/trace-capture.sw.yaml"),
    )?;
    let mut runtime = TraceCapture {
        console,
        output,
        seconds,
    };
    execute_workflow(
        &workflow,
        &mut runtime,
        &json!({
            "trace_started": false,
            "timed_mode": seconds.is_some(),
            "self_test": self_test,
            "recording_seen": false,
            "stop_requested": false
        }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_dump() -> Vec<String> {
        let mut events = vec![
            "TRACE EVENT seq=0 ts=1 core=0 context_kind=task context_id=1 target=firmware name=capture_started".to_owned(),
            "TRACE EVENT seq=1 ts=2 core=0 context_kind=task context_id=1 target=background name=allowed".to_owned(),
        ];
        let mut sequence = 2;
        for producer in 0_u64..2 {
            for index in 0_u64..16 {
                let value = (producer << 32) | index;
                events.push(format!(
                    "TRACE EVENT seq={sequence} ts={} core={producer} context_kind=task context_id={} target=firmware name=probe f.producer=u:{producer} f.index=u:{index} f.value=u:{value} f.inverse=u:{}",
                    sequence + 10,
                    producer + 1,
                    !value
                ));
                sequence += 1;
            }
        }
        let mut lines = vec![format!(
            "TRACE BEGIN version=1 generation=9 count={} overflow=0 unsupported=0 hook_failures=0 field_overflow=0 parented=0 formatting_errors=0 buffer_full=0",
            events.len()
        )];
        lines.extend(events);
        lines.push("TRACE END generation=9".to_owned());
        lines
    }

    #[test]
    fn parses_complete_dump_with_strict_records() -> Result<()> {
        let lines = vec![
            "TRACE BEGIN version=1 generation=7 count=1 overflow=0 unsupported=0 hook_failures=0 field_overflow=2 parented=0".into(),
            "TRACE EVENT seq=4 ts=88 core=1 context_kind=task context_id=2 target=interaction name=stage f.flow_id=u:9 f.ok=b:1 f.ratio=f:1.25e-1".into(),
            "TRACE END generation=7".into(),
        ];
        assert_eq!(parse_dump(&lines)?, (7, 1, true));
        assert!(parse_dump(&["TRACE START OK extra".into()]).is_err());
        let duplicate = vec![
            "TRACE BEGIN version=1 generation=7 count=2 overflow=0 unsupported=0 hook_failures=0"
                .into(),
            "TRACE EVENT seq=4 ts=88 core=1 context_kind=task context_id=2 target=x name=y".into(),
            "TRACE EVENT seq=4 ts=89 core=1 context_kind=task context_id=2 target=x name=z".into(),
            "TRACE END generation=7".into(),
        ];
        assert!(parse_dump(&duplicate).is_err());
        let nonfinite = vec![
            "TRACE BEGIN version=1 generation=7 count=1 overflow=0 unsupported=0 hook_failures=0".into(),
            "TRACE EVENT seq=0 ts=88 core=1 context_kind=task context_id=2 target=x name=y f.ratio=f:1e999".into(),
            "TRACE END generation=7".into(),
        ];
        assert!(parse_dump(&nonfinite).is_err());
        Ok(())
    }

    #[test]
    fn verifies_complete_probe_matrix_and_rejects_damage_or_missing_producer() -> Result<()> {
        let complete = probe_dump();
        verify_probe_lines(&complete)?;

        let mut damaged = complete.clone();
        let row = damaged
            .iter_mut()
            .find(|line| line.contains("f.producer=u:1 f.index=u:7"))
            .expect("producer one row");
        *row = row.replace("f.value=u:4294967303", "f.value=u:4294967304");
        assert!(verify_probe_lines(&damaged).is_err());

        let mut missing = complete;
        missing.retain(|line| !line.contains("target=firmware name=probe f.producer=u:1"));
        let count = missing.len() - 2;
        missing[0] = missing[0].replace("count=34", &format!("count={count}"));
        let end_index = missing.len() - 1;
        for (sequence, line) in missing[1..end_index].iter_mut().enumerate() {
            let end = line.find(" ts=").expect("event sequence delimiter");
            line.replace_range("TRACE EVENT seq=".len()..end, &sequence.to_string());
        }
        assert!(verify_probe_lines(&missing).is_err());
        Ok(())
    }

    #[test]
    fn action_scenario_waits_while_armed_and_capturing_then_stops() -> Result<()> {
        #[derive(Default)]
        struct Probe {
            calls: Vec<String>,
            statuses: usize,
            waits: usize,
        }
        impl WorkflowRuntime for Probe {
            fn invoke(&mut self, action: &str, _: &Value, context: &mut Value) -> Result<()> {
                self.calls.push(action.into());
                match action {
                    "arm_trace" => {
                        context["trace_started"] = json!(true);
                        context["trace_state"] = json!("armed");
                    }
                    "poll_status" => {
                        context["trace_state"] = json!(if self.statuses < 5000 {
                            "armed"
                        } else {
                            "capturing"
                        });
                        self.statuses += 1;
                    }
                    "wait_poll" => {
                        self.waits += 1;
                        context["stop_requested"] = json!(self.waits == 10000);
                    }
                    "mark_recording" => context["recording_seen"] = json!(true),
                    _ => {}
                }
                Ok(())
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/trace-capture.sw.yaml"),
        )?;
        let mut probe = Probe::default();
        let context = execute_workflow(
            &workflow,
            &mut probe,
            &json!({"trace_started": false, "timed_mode": false, "self_test": false, "recording_seen": false, "stop_requested": false}),
        )?;
        assert!(context.get("capture_error").is_none(), "{context}");
        assert_eq!(probe.statuses, 10000);
        assert!(probe.calls.contains(&"mark_recording".into()));
        assert!(probe.calls.ends_with(&[
            "stop_trace".into(),
            "dump_trace".into(),
            "finish".into()
        ]));
        Ok(())
    }

    #[test]
    fn action_scenario_marks_full_and_runs_cleanup() -> Result<()> {
        #[derive(Default)]
        struct Probe(Vec<String>);
        impl WorkflowRuntime for Probe {
            fn invoke(&mut self, action: &str, _: &Value, context: &mut Value) -> Result<()> {
                self.0.push(action.into());
                match action {
                    "arm_trace" => {
                        context["trace_started"] = json!(true);
                        context["trace_state"] = json!("armed");
                    }
                    "poll_status" => context["trace_state"] = json!("full"),
                    "mark_frozen" => context["trace_full"] = json!(true),
                    _ => {}
                }
                Ok(())
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/trace-capture.sw.yaml"),
        )?;
        let mut probe = Probe::default();
        let context = execute_workflow(
            &workflow,
            &mut probe,
            &json!({"trace_started": false, "timed_mode": false, "self_test": false, "recording_seen": false, "stop_requested": false}),
        )?;
        assert_eq!(context["trace_full"], true);
        assert!(probe.0.contains(&"mark_frozen".into()));
        assert!(probe
            .0
            .ends_with(&["stop_trace".into(), "dump_trace".into(), "finish".into()]));
        Ok(())
    }

    #[test]
    fn self_test_scenario_skips_arm_loop_and_verifies_after_dump() -> Result<()> {
        #[derive(Default)]
        struct Probe(Vec<String>);
        impl WorkflowRuntime for Probe {
            fn invoke(&mut self, action: &str, _: &Value, context: &mut Value) -> Result<()> {
                self.0.push(action.into());
                if action == "self_test" {
                    context["trace_started"] = json!(true);
                }
                Ok(())
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/trace-capture.sw.yaml"),
        )?;
        let mut probe = Probe::default();
        execute_workflow(
            &workflow,
            &mut probe,
            &json!({"trace_started": false, "timed_mode": false, "self_test": true}),
        )?;
        assert!(probe.0.contains(&"self_test".into()));
        assert!(!probe.0.contains(&"arm_trace".into()));
        assert!(!probe.0.contains(&"poll_status".into()));
        let dump = probe
            .0
            .iter()
            .position(|call| call == "dump_trace")
            .unwrap();
        let verify = probe
            .0
            .iter()
            .position(|call| call == "verify_probe")
            .unwrap();
        assert!(dump < verify);
        Ok(())
    }
}
