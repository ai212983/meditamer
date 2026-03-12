use std::{
    thread,
    time::{Duration as StdDuration, Instant},
};

use anyhow::Error;
use serde_json::json;
use serverless_workflow_core::models::retry::{
    OneOfRetryPolicyDefinitionOrReference, RetryPolicyDefinition,
};

fn execute_try_task<R: WorkflowRuntime>(
    task: &TryTaskDefinition,
    runtime: &mut R,
    context: &mut Value,
) -> Result<Option<String>> {
    if !should_run(&task.common, context)? {
        return Ok(task.common.then.clone());
    }

    let error_var = task.catch.as_.as_deref().unwrap_or("error");
    let retry_policy = resolve_retry_policy(task, context)?;
    let mut retry_attempt = 0u16;
    let started = Instant::now();

    loop {
        match execute_task_map(&task.try_, runtime, context) {
            Ok(()) => return Ok(task.common.then.clone()),
            Err(err) => {
                bind_workflow_error(context, error_var, &err, retry_attempt)?;
                if !catch_matches(task, context, error_var)? {
                    return Err(err);
                }
                if should_retry(task, retry_policy.as_ref(), context, retry_attempt, started)? {
                    retry_attempt = retry_attempt.saturating_add(1);
                    sleep_before_retry(retry_policy.as_ref(), retry_attempt);
                    continue;
                }
                if let Some(tasks) = &task.catch.do_ {
                    execute_task_map(tasks, runtime, context)?;
                }
                return Ok(task.common.then.clone());
            }
        }
    }
}

fn bind_workflow_error(
    context: &mut Value,
    error_var: &str,
    err: &Error,
    retry_attempt: u16,
) -> Result<()> {
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

fn catch_matches(task: &TryTaskDefinition, context: &Value, error_var: &str) -> Result<bool> {
    if let Some(filter) = &task.catch.errors {
        if !error_matches_filter(filter, context, error_var)? {
            return Ok(false);
        }
    }
    if let Some(condition) = &task.catch.when {
        if !eval_condition(condition, context)? {
            return Ok(false);
        }
    }
    if let Some(condition) = &task.catch.except_when {
        if eval_condition(condition, context)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn resolve_retry_policy(task: &TryTaskDefinition, context: &Value) -> Result<Option<RetryPolicy>> {
    let Some(retry) = &task.catch.retry else {
        return Ok(None);
    };
    let policy = match retry {
        OneOfRetryPolicyDefinitionOrReference::Retry(policy) => policy,
        OneOfRetryPolicyDefinitionOrReference::Reference(name) => {
            return Err(anyhow!("workflow retry references are not supported yet: {name}"));
        }
    };
    if policy.backoff.is_some() {
        return Err(anyhow!("workflow retry.backoff is not supported yet"));
    }
    if policy.jitter.is_some() {
        return Err(anyhow!("workflow retry.jitter is not supported yet"));
    }

    let metadata = resolve_retry_metadata(&task.common, context)?;
    let max_retries = metadata.count.or_else(|| retry_limit_count(policy)).unwrap_or(1);
    let delay = metadata
        .delay
        .or_else(|| retry_delay(policy))
        .unwrap_or_default();

    if retry_attempt_duration(policy).is_some() {
        return Err(anyhow!("workflow retry.limit.attempt.duration is not supported yet"));
    }

    Ok(Some(RetryPolicy {
        max_retries,
        delay,
        window: retry_duration_limit(policy),
        when: policy.when.clone(),
        except_when: policy.except_when.clone(),
    }))
}

fn should_retry(
    task: &TryTaskDefinition,
    retry_policy: Option<&RetryPolicy>,
    context: &Value,
    retry_attempt: u16,
    started: Instant,
) -> Result<bool> {
    let Some(policy) = retry_policy else {
        return Ok(false);
    };
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
    catch_matches(task, context, task.catch.as_.as_deref().unwrap_or("error"))
}

fn sleep_before_retry(retry_policy: Option<&RetryPolicy>, retry_attempt: u16) {
    let Some(delay) = retry_policy.map(|policy| policy.delay) else {
        return;
    };
    if delay.is_zero() {
        return;
    }
    thread::sleep(delay.saturating_mul(retry_attempt as u32));
}

fn resolve_retry_metadata(common: &TaskDefinitionFields, context: &Value) -> Result<RetryMetadata> {
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
            .map(StdDuration::from_millis),
    })
}

fn retry_limit_count(policy: &RetryPolicyDefinition) -> Option<u16> {
    policy
        .limit
        .as_ref()
        .and_then(|limit| limit.attempt.as_ref())
        .and_then(|attempt| attempt.count)
}

fn retry_attempt_duration(policy: &RetryPolicyDefinition) -> Option<StdDuration> {
    policy
        .limit
        .as_ref()
        .and_then(|limit| limit.attempt.as_ref())
        .and_then(|attempt| attempt.duration.as_ref())
        .map(duration_to_std)
}

fn retry_duration_limit(policy: &RetryPolicyDefinition) -> Option<StdDuration> {
    policy
        .limit
        .as_ref()
        .and_then(|limit| limit.duration.as_ref())
        .map(duration_to_std)
}

fn retry_delay(policy: &RetryPolicyDefinition) -> Option<StdDuration> {
    policy.delay.as_ref().map(duration_to_std)
}

fn duration_to_std(duration: &serverless_workflow_core::models::duration::Duration) -> StdDuration {
    StdDuration::from_millis(duration.total_milliseconds())
}

#[derive(Clone)]
struct RetryPolicy {
    max_retries: u16,
    delay: StdDuration,
    window: Option<StdDuration>,
    when: Option<String>,
    except_when: Option<String>,
}

#[derive(Default)]
struct RetryMetadata {
    count: Option<u16>,
    delay: Option<StdDuration>,
}
