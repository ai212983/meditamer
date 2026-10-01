use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PsramSample {
    pub label: String,
    pub raw_line: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointEntry {
    pub name: String,
    pub status: String,
    pub elapsed_ms: Option<u64>,
    pub marker_line: Option<String>,
    pub psram: Option<PsramSample>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetResidencyReport {
    pub feature_label: String,
    pub build_label: String,
    pub started_at: String,
    pub finished_at: String,
    pub caught_error: Option<String>,
    pub checkpoints: Vec<CheckpointEntry>,
    pub state_lines: Vec<String>,
    pub run_passed: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct CheckpointState {
    pub(super) status: String,
    pub(super) elapsed_ms: Option<u64>,
    pub(super) marker_line: Option<String>,
    pub(super) psram: Option<PsramSample>,
}

/// Parse a `PSRAM k=v ...` line by exact prefix and whitespace-separated
/// `key=value` tokens. Unknown fields are kept; tokens without `=` are
/// skipped. Lines without the prefix are rejected.
pub fn parse_psram_line(label: &str, line: &str) -> Result<PsramSample> {
    let rest = line
        .strip_prefix("PSRAM")
        .ok_or_else(|| anyhow!("not a PSRAM line: {line}"))?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return Err(anyhow!("not a PSRAM line: {line}"));
    }
    let mut fields = BTreeMap::new();
    for token in rest.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        fields.insert(key.to_string(), value.to_string());
    }
    Ok(PsramSample {
        label: label.to_string(),
        raw_line: line.to_string(),
        fields,
    })
}

pub(super) fn report_path_for(log_path: &Path) -> PathBuf {
    let mut path = log_path.to_path_buf();
    // A caller may choose a .json capture filename. Never overwrite that
    // capture with the machine-readable report at the end of the run.
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        path.set_extension("report.json");
    } else {
        path.set_extension("json");
    }
    path
}

pub(super) fn caught_message(context: &Value) -> Option<String> {
    context
        .get("measurement_error")
        .map(|value| match value {
            Value::String(message) => message.clone(),
            Value::Object(map) => map
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string()),
            _ => value.to_string(),
        })
        .filter(|message| !message.is_empty())
}

pub(super) fn all_required_pass(
    checkpoints: &HashMap<String, CheckpointState>,
    required: &[String],
) -> bool {
    required.iter().all(|name| {
        checkpoints
            .get(name)
            .is_some_and(|state| checkpoint_satisfied(name, &state.status))
    })
}

pub(super) fn checkpoint_satisfied(name: &str, status: &str) -> bool {
    status == "pass"
        || (status == "inferred"
            && (matches!(
                name,
                "mountain_validated"
                    | "mountain_rendered"
                    | "clock1_assets_ready"
                    | "clock2_assets_ready"
            ) || (name.starts_with("cycle_")
                && [
                    "_mountain_cache_validated",
                    "_mountain_validated",
                    "_mountain_rendered",
                    "_clock_assets_ready",
                ]
                .iter()
                .any(|suffix| name.ends_with(suffix)))))
}

pub(super) fn assemble_report(
    feature_label: &str,
    build_label: &str,
    started_at: &str,
    required: &[String],
    checkpoints: &HashMap<String, CheckpointState>,
    state_lines: &[String],
    caught_error: Option<String>,
) -> AssetResidencyReport {
    let run_passed = caught_error.is_none() && all_required_pass(checkpoints, required);
    let checkpoints = required
        .iter()
        .map(|name| {
            let state = checkpoints.get(name);
            CheckpointEntry {
                name: name.clone(),
                status: state
                    .map(|state| state.status.clone())
                    .unwrap_or_else(|| "missing".to_string()),
                elapsed_ms: state.and_then(|state| state.elapsed_ms),
                marker_line: state.and_then(|state| state.marker_line.clone()),
                psram: state.and_then(|state| state.psram.clone()),
            }
        })
        .collect();
    AssetResidencyReport {
        feature_label: feature_label.to_string(),
        build_label: build_label.to_string(),
        started_at: started_at.to_string(),
        finished_at: chrono::Utc::now().to_rfc3339(),
        caught_error,
        checkpoints,
        state_lines: state_lines.to_vec(),
        run_passed,
    }
}
