const WORKFLOW_END_TRANSITION: &str = "__end__";

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
            if next_name == WORKFLOW_END_TRANSITION {
                return Ok(());
            }
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

    let output = runtime.invoke_with_result(&task.call, &Value::Object(args), context)?;
    bind_call_result(&task.common, context, output)?;
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
    if execute_repeatable_do_task(task, runtime, context)? {
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

    let mut default_then = None;
    for entry in &task.switch.entries {
        let Some((name, case)) = entry.iter().next() else {
            continue;
        };
        if name == "default" {
            if default_then.is_some() {
                return Err(anyhow!("workflow switch defines multiple default cases"));
            }
            default_then = Some(case.then.clone().or_else(|| task.common.then.clone()));
            continue;
        }
        let matches = match &case.when {
            Some(condition) => eval_condition(condition, context)?,
            None => true,
        };
        if matches {
            return Ok(case.then.clone().or_else(|| task.common.then.clone()));
        }
    }

    Ok(default_then.unwrap_or_else(|| task.common.then.clone()))
}
include!("engine_repeat.rs");
include!("engine_result.rs");
include!("engine_retry.rs");
include!("engine_support.rs");
