use super::capture::{write_summary, SummaryInputs};
use super::FlashCaptureRuntime;

use anyhow::{anyhow, Result};
use serde_json::Value;

pub(super) fn action_write_summary(
    runtime: &mut FlashCaptureRuntime<'_>,
    context: &mut Value,
) -> Result<()> {
    let result = runtime
        .flash_result
        .as_ref()
        .ok_or_else(|| anyhow!("cannot write summary without flash result"))?;
    write_summary(SummaryInputs {
        outputs: &runtime.outputs,
        port: &runtime.port,
        baud: runtime.baud,
        flash_baud: runtime.flash_baud,
        result,
        capture_mode: runtime.opts.capture_mode,
        capture_bytes: runtime.capture_bytes,
        post_command: runtime.opts.post_command.as_deref(),
        post_command_match: runtime.post_command_match.as_deref(),
        time_sync_status: runtime.time_sync_status.label(),
        time_sync_utc: runtime.time_sync_utc,
        time_sync_offset_minutes: runtime.time_sync_offset_minutes,
        time_sync_reason: runtime.time_sync_reason.as_deref(),
    })?;
    context_set_string(
        context,
        "artifact_root",
        &runtime.outputs.root.display().to_string(),
    );
    Ok(())
}

pub(super) fn action_abort_flash(context: &Value) -> Result<()> {
    let detail = context
        .get("flash_error")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("flash failed");
    Err(anyhow!("{detail}"))
}

/// Terminal action for a failed `time_sync`: reached only *after*
/// `write_summary` has already recorded `time_sync=failed` to disk, so the
/// flash result is retained and only the workflow's final exit code turns
/// nonzero (`flash=ok time_sync=failed`).
pub(super) fn action_fail_time_sync(context: &Value) -> Result<()> {
    let reason = context
        .get("time_sync_reason")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    Err(anyhow!("flash=ok time_sync=failed reason={reason}"))
}

pub(super) fn context_set_string(context: &mut Value, key: &str, value: &str) {
    if let Some(map) = context.as_object_mut() {
        map.insert(key.to_string(), Value::String(value.to_string()));
    }
}

pub(super) fn context_set_u64(context: &mut Value, key: &str, value: u64) {
    if let Some(map) = context.as_object_mut() {
        map.insert(key.to_string(), Value::Number(value.into()));
    }
}

pub(super) fn context_set_bool(context: &mut Value, key: &str, value: bool) {
    if let Some(map) = context.as_object_mut() {
        map.insert(key.to_string(), Value::Bool(value));
    }
}
