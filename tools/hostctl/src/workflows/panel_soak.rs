//! Production-image refresh capture. Repetition and gates belong to the YAML.
use crate::{
    env_utils,
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::SerialConsole,
    workflows::{observation_fixture::generated_request_id, wifi::common::acquire_port_lock},
};
use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Capture {
    console: SerialConsole,
    output: PathBuf,
    cursor: usize,
    completed: u16,
    partials: u16,
    requests: Vec<u64>,
}

impl Capture {
    fn wait(&mut self, pattern: &str, seconds: u64) -> Result<String> {
        let regex = Regex::new(pattern)?;
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            if self.output.join("STOP").exists() {
                bail!("operator stop requested");
            }
            self.console.poll_once()?;
            let lines = self.console.read_recent_lines(self.cursor);
            self.cursor = self.console.mark();
            let mut matched = None;
            for line in lines {
                if [
                    "Guru Meditation",
                    "panicked at",
                    "rst:",
                    "status=error",
                    "PANEL_BUS_SUSPEND",
                    "PANEL_BUS_RESUME",
                ]
                .iter()
                .any(|marker| line.contains(marker))
                {
                    bail!("device failure: {line}");
                }
                if regex.is_match(&line) {
                    matched = Some(line);
                }
            }
            if let Some(line) = matched {
                return Ok(line);
            }
        }
        bail!("timed out waiting for {pattern}; no command retry sent")
    }

    fn progress(&self, context: &Value) -> Result<()> {
        let data = json!({
            "completed_full_refreshes": self.completed,
            "preceding_partial_refreshes": self.partials,
            "request_ids": self.requests,
            "visual_status": "requires_physical_observation",
            "context": context,
        });
        let temp = self.output.join("progress.tmp");
        fs::write(&temp, serde_json::to_vec_pretty(&data)?)?;
        fs::rename(temp, self.output.join("progress.json"))?;
        Ok(())
    }
}

impl WorkflowRuntime for Capture {
    fn invoke(&mut self, action: &str, _: &Value, context: &mut Value) -> Result<()> {
        match action {
            "ready" => {
                self.console.send_line("STATE GET")?;
                self.wait(
                    r"^STATE phase=OPERATING upload=(on|off) diag_kind=NONE targets=NONE ready=true$",
                    10,
                )?;
            }
            "wait_partial" => {
                self.wait(r"^LVGL_REFRESH .*status=ok kind=partial ", 90)?;
                self.partials += 1;
            }
            "repaint" => {
                if self.output.join("STOP").exists() {
                    bail!("operator stop requested");
                }
                let id = generated_request_id()?;
                self.requests.push(id);
                self.progress(context)?;
                self.console.send_line(&format!("REPAINT {id}"))?;
                let line = self.wait(
                    &format!(r"^PANEL_FIXTURE END id={id} status=\w+ at_ms=\d+$"),
                    20,
                )?;
                if !line.contains("status=Completed ") {
                    bail!("refresh failed: {line}");
                }
                self.completed += 1;
            }
            "progress" | "finish" => self.progress(context)?,
            "fail" => {
                return Err(anyhow!(
                    "panel capture stopped: {}",
                    context["capture_error"]
                ))
            }
            _ => bail!("unknown panel-soak action: {action}"),
        }
        Ok(())
    }
}

pub fn run(cycles: u16, wait_partial: bool, output: PathBuf) -> Result<()> {
    if !(1..=500).contains(&cycles) {
        bail!("cycles must be in 1..=500");
    }
    fs::create_dir_all(&output)?;
    let port = env_utils::require_port()?;
    let _lock = acquire_port_lock(&port)?;
    let console = SerialConsole::open_passive(
        &port,
        env_utils::baud_from_env(115_200)?,
        Some(&output.join("serial.log")),
    )?;
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/panel-soak.sw.yaml"),
    )?;
    let mut capture = Capture {
        console,
        output,
        cursor: 0,
        completed: 0,
        partials: 0,
        requests: Vec::new(),
    };
    execute_workflow(
        &workflow,
        &mut capture,
        &json!({"cycles": cycles, "wait_partial": wait_partial, "capture_complete": false}),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        calls: Vec<String>,
        fail_refresh: bool,
    }
    impl WorkflowRuntime for Fake {
        fn invoke(&mut self, action: &str, _: &Value, _: &mut Value) -> Result<()> {
            self.calls.push(action.into());
            if action == "fail" || (action == "repaint" && self.fail_refresh) {
                bail!("test failure");
            }
            Ok(())
        }
    }
    #[test]
    fn workflow_pairs_refreshes_and_never_retries_ambiguous_completion() {
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/panel-soak.sw.yaml"),
        )
        .unwrap();
        for paired in [false, true] {
            let mut fake = Fake {
                calls: Vec::new(),
                fail_refresh: false,
            };
            execute_workflow(
                &workflow,
                &mut fake,
                &json!({"cycles": 3, "wait_partial": paired, "capture_complete": false}),
            )
            .unwrap();
            let mut expected = vec!["ready"];
            for _ in 0..3 {
                if paired {
                    expected.push("wait_partial");
                }
                expected.extend(["repaint", "progress"]);
            }
            expected.push("finish");
            assert_eq!(fake.calls, expected);
        }
        let mut fake = Fake {
            calls: Vec::new(),
            fail_refresh: true,
        };
        assert!(execute_workflow(
            &workflow,
            &mut fake,
            &json!({"cycles": 3, "wait_partial": false, "capture_complete": false})
        )
        .is_err());
        assert_eq!(fake.calls, ["ready", "repaint", "finish", "fail"]);
    }
}
