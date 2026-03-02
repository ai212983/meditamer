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
            TaskDefinition::Switch(def) => execute_switch_task(def, context)?,
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
            args.insert(key.clone(), value.clone());
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

fn should_run(common: &TaskDefinitionFields, context: &Value) -> Result<bool> {
    match &common.if_ {
        Some(condition) => eval_condition(condition, context),
        None => Ok(true),
    }
}

fn task_type_name(task: &TaskDefinition) -> &'static str {
    match task {
        TaskDefinition::Call(_) => "call",
        TaskDefinition::Do(_) => "do",
        TaskDefinition::Emit(_) => "emit",
        TaskDefinition::For(_) => "for",
        TaskDefinition::Fork(_) => "fork",
        TaskDefinition::Listen(_) => "listen",
        TaskDefinition::Raise(_) => "raise",
        TaskDefinition::Run(_) => "run",
        TaskDefinition::Set(_) => "set",
        TaskDefinition::Switch(_) => "switch",
        TaskDefinition::Try(_) => "try",
        TaskDefinition::Wait(_) => "wait",
    }
}

struct TaskIndex {
    tasks_by_name: HashMap<String, TaskDefinition>,
    ordered_names: Vec<String>,
    position: HashMap<String, usize>,
}

fn index_tasks(tasks: &WorkflowMap<String, TaskDefinition>) -> Result<TaskIndex> {
    let mut tasks_by_name = HashMap::new();
    let mut ordered_names = Vec::new();

    for entry in &tasks.entries {
        if entry.len() != 1 {
            return Err(anyhow!(
                "each task entry must contain exactly one task name/definition pair"
            ));
        }

        let Some((name, task)) = entry.iter().next() else {
            continue;
        };
        if tasks_by_name.insert(name.clone(), task.clone()).is_some() {
            return Err(anyhow!("duplicate task name '{name}' in workflow"));
        }
        ordered_names.push(name.clone());
    }

    let position = ordered_names
        .iter()
        .enumerate()
        .map(|(idx, name)| (name.clone(), idx))
        .collect::<HashMap<_, _>>();

    Ok(TaskIndex {
        tasks_by_name,
        ordered_names,
        position,
    })
}
