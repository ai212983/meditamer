fn execute_call_task<R: WorkflowRuntime>(
    task: &CallTaskDefinition,
    runtime: &mut R,
    context: &mut Value,
) -> Result<Option<String>> {
    if !should_run(&task.common, context)? {
        return Ok(task.common.then.clone());
    }

    let args = resolve_call_args(task, context)?;
    let retry_policy = resolve_call_retry_policy(&task.common, context)?;
    let error_var = retry_policy
        .as_ref()
        .and_then(|policy| policy.error_var.as_deref())
        .unwrap_or("error");
    let mut retry_attempt = 0u16;
    let started = std::time::Instant::now();

    loop {
        match runtime.invoke_with_result(&task.call, &args, context) {
            Ok(output) => {
                bind_call_result(&task.common, context, output)?;
                return Ok(task.common.then.clone());
            }
            Err(err) => {
                let should_retry = if let Some(policy) = &retry_policy {
                    bind_workflow_error(context, error_var, &err, retry_attempt)?;
                    retry_policy_allows_retry(policy, context, retry_attempt, started)?
                } else {
                    false
                };
                if should_retry {
                    retry_attempt = retry_attempt.saturating_add(1);
                    sleep_before_retry(retry_policy.as_ref().expect("retry policy exists"), retry_attempt);
                    continue;
                }
                return Err(err);
            }
        }
    }
}

fn resolve_call_args(task: &CallTaskDefinition, context: &Value) -> Result<Value> {
    let mut args = JsonMap::new();
    if let Some(with) = &task.with {
        for (key, value) in with {
            args.insert(key.clone(), resolve_runtime_value(value, context)?);
        }
    }
    Ok(Value::Object(args))
}

fn resolve_call_retry_policy(common: &TaskDefinitionFields, context: &Value) -> Result<Option<RetryPolicy>> {
    let metadata = resolve_retry_metadata(common, context)?;
    let Some(max_retries) = metadata.count else {
        return Ok(None);
    };
    Ok(Some(RetryPolicy {
        max_retries,
        delay: metadata.delay.unwrap_or_default(),
        window: None,
        when: metadata.when,
        except_when: metadata.except_when,
        error_var: metadata.error_var,
    }))
}
