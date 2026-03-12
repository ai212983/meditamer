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
    let started = std::time::Instant::now();

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
                    sleep_before_retry(retry_policy.as_ref().expect("retry policy exists"), retry_attempt);
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
        error_var: None,
    }))
}

fn should_retry(
    task: &TryTaskDefinition,
    retry_policy: Option<&RetryPolicy>,
    context: &Value,
    retry_attempt: u16,
    started: std::time::Instant,
) -> Result<bool> {
    let Some(policy) = retry_policy else {
        return Ok(false);
    };
    if !retry_policy_allows_retry(policy, context, retry_attempt, started)? {
        return Ok(false);
    }
    catch_matches(task, context, task.catch.as_.as_deref().unwrap_or("error"))
}

fn retry_limit_count(policy: &RetryPolicyDefinition) -> Option<u16> {
    policy
        .limit
        .as_ref()
        .and_then(|limit| limit.attempt.as_ref())
        .and_then(|attempt| attempt.count)
}

fn retry_attempt_duration(policy: &RetryPolicyDefinition) -> Option<std::time::Duration> {
    policy
        .limit
        .as_ref()
        .and_then(|limit| limit.attempt.as_ref())
        .and_then(|attempt| attempt.duration.as_ref())
        .map(duration_to_std)
}

fn retry_duration_limit(policy: &RetryPolicyDefinition) -> Option<std::time::Duration> {
    policy
        .limit
        .as_ref()
        .and_then(|limit| limit.duration.as_ref())
        .map(duration_to_std)
}

fn retry_delay(policy: &RetryPolicyDefinition) -> Option<std::time::Duration> {
    policy.delay.as_ref().map(duration_to_std)
}

fn duration_to_std(duration: &serverless_workflow_core::models::duration::Duration) -> std::time::Duration {
    std::time::Duration::from_millis(duration.total_milliseconds())
}
