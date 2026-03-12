fn execute_task_map<R: WorkflowRuntime>(
    tasks: &WorkflowMap<String, TaskDefinition>,
    runtime: &mut R,
    context: &mut Value,
) -> Result<()> {
    let TaskIndex {
        tasks_by_name,
        ordered_names,
        position,
    } = index_tasks(tasks)?;

    if ordered_names.is_empty() {
        return Ok(());
    }

    let mut current = ordered_names[0].clone();
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 4096 {
            return Err(anyhow!("workflow exceeded maximum transition depth"));
        }

        let task = tasks_by_name
            .get(&current)
            .ok_or_else(|| anyhow!("missing task definition for '{current}'"))?;

        let next = match task {
            TaskDefinition::Call(def) => execute_call_task(def, runtime, context)?,
            TaskDefinition::Do(def) => execute_do_task(def, runtime, context)?,
            TaskDefinition::Set(def) => execute_set_task(def, context)?,
            TaskDefinition::Switch(def) => execute_switch_task(def, context)?,
            TaskDefinition::Try(def) => execute_try_task(def, runtime, context)?,
            other => {
                return Err(anyhow!(
                    "task '{current}' uses unsupported task type '{}'",
                    task_type_name(other)
                ))
            }
        };

        if let Some(next_name) = next {
            if tasks_by_name.contains_key(&next_name) {
                current = next_name;
                continue;
            }
            return Err(anyhow!(
                "task '{current}' transitions to unknown task '{next_name}'"
            ));
        }

        let index = *position
            .get(&current)
            .ok_or_else(|| anyhow!("missing task index for '{current}'"))?;
        if index + 1 >= ordered_names.len() {
            return Ok(());
        }
        current = ordered_names[index + 1].clone();
    }
}

fn execute_call_task<R: WorkflowRuntime>(
    task: &CallTaskDefinition,
    runtime: &mut R,
    context: &mut Value,
) -> Result<Option<String>> {
    if !should_run(&task.common, context)? {
        return Ok(task.common.then.clone());
    }

    let mut args = JsonMap::new();
    if let Some(with) = &task.with {
        for (key, value) in with {
            args.insert(key.clone(), resolve_runtime_value(value, context)?);
        }
    }

    runtime.invoke(&task.call, &Value::Object(args), context)?;
    Ok(task.common.then.clone())
}

fn execute_do_task<R: WorkflowRuntime>(
    task: &DoTaskDefinition,
    runtime: &mut R,
    context: &mut Value,
) -> Result<Option<String>> {
    if !should_run(&task.common, context)? {
        return Ok(task.common.then.clone());
    }
    execute_task_map(&task.do_, runtime, context)?;
    Ok(task.common.then.clone())
}

fn execute_set_task(task: &SetTaskDefinition, context: &mut Value) -> Result<Option<String>> {
    if !should_run(&task.common, context)? {
        return Ok(task.common.then.clone());
    }

    for (key, value) in &task.set {
        set_context_path(context, key, resolve_runtime_value(value, context)?)?;
    }

    Ok(task.common.then.clone())
}

fn execute_switch_task(task: &SwitchTaskDefinition, context: &Value) -> Result<Option<String>> {
    if !should_run(&task.common, context)? {
        return Ok(task.common.then.clone());
    }

    for entry in &task.switch.entries {
        let Some((_, case)) = entry.iter().next() else {
            continue;
        };
        let matches = match &case.when {
            Some(condition) => eval_condition(condition, context)?,
            None => true,
        };
        if matches {
            return Ok(case.then.clone().or_else(|| task.common.then.clone()));
        }
    }

    Ok(task.common.then.clone())
}

fn execute_try_task<R: WorkflowRuntime>(
    task: &TryTaskDefinition,
    runtime: &mut R,
    context: &mut Value,
) -> Result<Option<String>> {
    if !should_run(&task.common, context)? {
        return Ok(task.common.then.clone());
    }

    match execute_task_map(&task.try_, runtime, context) {
        Ok(()) => Ok(task.common.then.clone()),
        Err(err) => {
            if task.catch.retry.is_some() {
                return Err(anyhow!("workflow catch.retry is not supported yet"));
            }

            let error_var = task.catch.as_.as_deref().unwrap_or("error");
            let error_value = serde_json::json!({
                "message": err.to_string(),
            });
            set_context_path(context, error_var, error_value)?;

            if let Some(filter) = &task.catch.errors {
                if !error_matches_filter(filter, context, error_var)? {
                    return Err(err);
                }
            }
            if let Some(condition) = &task.catch.when {
                if !eval_condition(condition, context)? {
                    return Err(err);
                }
            }
            if let Some(condition) = &task.catch.except_when {
                if eval_condition(condition, context)? {
                    return Err(err);
                }
            }

            if let Some(tasks) = &task.catch.do_ {
                execute_task_map(tasks, runtime, context)?;
            }
            Ok(task.common.then.clone())
        }
    }
}

include!("engine_support.rs");
