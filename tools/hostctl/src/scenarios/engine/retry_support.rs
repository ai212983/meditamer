use crate::scenarios::conditions::eval_condition;
use crate::scenarios::engine::result::merge_call_result;
use crate::scenarios::engine::support::resolve_runtime_value;
use crate::scenarios::engine::support::set_context_path;
use crate::scenarios::WorkflowActionError;
use anyhow::Result;
use serde_json::Value;
use serverless_workflow_core::models::task::TaskDefinitionFields;

use std::thread;

use anyhow::Error;
use serde_json::json;

fn bind_workflow_error_context_patch(context: &mut Value, err: &Error) -> Result<()> {
    if let Some(patch) = err
        .downcast_ref::<WorkflowActionError>()
        .and_then(|error| error.context_patch().cloned())
    {
        merge_call_result(context, None, patch)?;
    }
    Ok(())
}

pub(crate) fn bind_workflow_error(
    context: &mut Value,
    error_var: &str,
    err: &Error,
    retry_attempt: u16,
) -> Result<()> {
    bind_workflow_error_context_patch(context, err)?;
    set_context_path(
        context,
        error_var,
        json!({
            "message": err.to_string(),
            "attempt": retry_attempt + 1,
            "retry_attempt": retry_attempt,
        }),
    )
}

pub(crate) fn retry_policy_allows_retry(
    policy: &RetryPolicy,
    context: &Value,
    retry_attempt: u16,
    started: std::time::Instant,
) -> Result<bool> {
    if retry_attempt >= policy.max_retries {
        return Ok(false);
    }
    if let Some(window) = policy.window {
        if started.elapsed() >= window {
            return Ok(false);
        }
    }
    if let Some(condition) = &policy.when {
        if !eval_condition(condition, context)? {
            return Ok(false);
        }
    }
    if let Some(condition) = &policy.except_when {
        if eval_condition(condition, context)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn sleep_before_retry(policy: &RetryPolicy, retry_attempt: u16) {
    if policy.delay.is_zero() {
        return;
    }
    thread::sleep(policy.delay.saturating_mul(retry_attempt as u32));
}

pub(crate) fn resolve_retry_metadata(
    common: &TaskDefinitionFields,
    context: &Value,
) -> Result<RetryMetadata> {
    let Some(hostctl) = common
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("hostctl"))
        .and_then(Value::as_object)
    else {
        return Ok(RetryMetadata::default());
    };
    let Some(retry) = hostctl.get("retry").and_then(Value::as_object) else {
        return Ok(RetryMetadata::default());
    };

    Ok(RetryMetadata {
        count: retry
            .get("count")
            .map(|value| resolve_runtime_value(value, context))
            .transpose()?
            .and_then(|value| value.as_u64())
            .map(|value| value as u16),
        delay: retry
            .get("delayMs")
            .map(|value| resolve_runtime_value(value, context))
            .transpose()?
            .and_then(|value| value.as_u64())
            .map(std::time::Duration::from_millis),
        when: retry
            .get("when")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        except_when: retry
            .get("exceptWhen")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        error_var: retry
            .get("as")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

#[derive(Clone)]
pub(crate) struct RetryPolicy {
    pub(crate) max_retries: u16,
    pub(crate) delay: std::time::Duration,
    pub(crate) window: Option<std::time::Duration>,
    pub(crate) when: Option<String>,
    pub(crate) except_when: Option<String>,
    pub(crate) error_var: Option<String>,
}

#[derive(Default)]
pub(crate) struct RetryMetadata {
    pub(crate) count: Option<u16>,
    pub(crate) delay: Option<std::time::Duration>,
    pub(crate) when: Option<String>,
    pub(crate) except_when: Option<String>,
    pub(crate) error_var: Option<String>,
}
