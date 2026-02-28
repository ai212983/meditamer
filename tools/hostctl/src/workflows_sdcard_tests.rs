use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;
use serde_json::{json, Value};

use crate::{
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    workflows_sdcard::resolve_templates,
};

struct TraceRuntime {
    traces: Vec<String>,
}

impl WorkflowRuntime for TraceRuntime {
    fn invoke(&mut self, action: &str, args: &Value, _context: &mut Value) -> Result<()> {
        match action {
            "run_step" => {
                let name = args
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unnamed");
                self.traces.push(format!("run_step:{name}"));
            }
            "raw_expect_pattern" => {
                let name = args
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unnamed");
                self.traces.push(format!("raw_expect_pattern:{name}"));
            }
            other => self.traces.push(other.to_string()),
        }
        Ok(())
    }
}

fn run_sdcard_fixture(suite: &str) -> Result<Vec<String>> {
    let scenario_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/sdcard-hw.sw.yaml");
    let workflow = load_workflow(&scenario_path)?;
    let mut runtime = TraceRuntime { traces: Vec::new() };
    let _ = execute_workflow(&workflow, &mut runtime, &json!({ "suite": suite }))?;
    Ok(runtime.traces)
}

#[test]
fn resolve_templates_replaces_tokens() -> Result<()> {
    let vars = HashMap::from([
        ("base_path".to_string(), "/sd123".to_string()),
        ("file".to_string(), "io.txt".to_string()),
        ("verify_lba".to_string(), "2048".to_string()),
    ]);

    let resolved = resolve_templates("SDFATWRITE {base_path}/{file} {verify_lba}", &vars)?;
    assert_eq!(resolved, "SDFATWRITE /sd123/io.txt 2048");
    Ok(())
}

#[test]
fn resolve_templates_errors_on_unknown_token() {
    let vars = HashMap::from([("base_path".to_string(), "/sd123".to_string())]);
    let err = resolve_templates("SDFATWRITE {base_path}/{missing}", &vars)
        .expect_err("missing placeholder must fail");
    assert!(err
        .to_string()
        .contains("unknown template variable '{missing}'"));
}

#[test]
fn sdcard_fixture_baseline_only_skips_burst_and_failures() -> Result<()> {
    let traces = run_sdcard_fixture("baseline")?;
    assert!(traces.iter().any(|entry| entry == "run_step:mkdir"));
    assert!(!traces.iter().any(|entry| entry == "burst_batch_start"));
    assert!(!traces
        .iter()
        .any(|entry| entry == "run_step:fail_mkdir_nonempty"));
    Ok(())
}

#[test]
fn sdcard_fixture_burst_only_skips_baseline_and_failures() -> Result<()> {
    let traces = run_sdcard_fixture("burst")?;
    assert!(traces.iter().any(|entry| entry == "burst_batch_start"));
    assert!(traces.iter().any(|entry| entry == "burst_batch_assert"));
    assert!(!traces.iter().any(|entry| entry == "run_step:mkdir"));
    assert!(!traces
        .iter()
        .any(|entry| entry == "run_step:fail_mkdir_nonempty"));
    Ok(())
}

#[test]
fn sdcard_fixture_all_runs_all_sections() -> Result<()> {
    let traces = run_sdcard_fixture("all")?;
    assert!(traces.iter().any(|entry| entry == "run_step:mkdir"));
    assert!(traces.iter().any(|entry| entry == "burst_batch_start"));
    assert!(traces
        .iter()
        .any(|entry| entry == "run_step:fail_mkdir_nonempty"));
    assert!(traces
        .iter()
        .any(|entry| entry == "raw_expect_pattern:fail_oversize_payload_cmd_err"));
    Ok(())
}
