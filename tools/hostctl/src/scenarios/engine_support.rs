fn should_run(common: &TaskDefinitionFields, context: &Value) -> Result<bool> {
    match &common.if_ {
        Some(condition) => eval_condition(condition, context),
        None => Ok(true),
    }
}

fn resolve_runtime_value(value: &Value, context: &Value) -> Result<Value> {
    match value {
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_runtime_value(item, context))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut resolved = JsonMap::new();
            for (key, value) in map {
                resolved.insert(key.clone(), resolve_runtime_value(value, context)?);
            }
            Ok(Value::Object(resolved))
        }
        Value::String(raw) => resolve_runtime_string(raw, context),
        other => Ok(other.clone()),
    }
}

fn resolve_runtime_string(raw: &str, context: &Value) -> Result<Value> {
    let trimmed = raw.trim();
    if trimmed.starts_with("${") && trimmed.ends_with('}') {
        return eval_expression_value(trimmed, context);
    }
    if trimmed.starts_with('.') && trimmed == raw {
        return eval_expression_value(trimmed, context);
    }
    if raw.contains("${") {
        return interpolate_runtime_string(raw, context).map(Value::String);
    }
    Ok(Value::String(raw.to_string()))
}

fn interpolate_runtime_string(raw: &str, context: &Value) -> Result<String> {
    let mut rendered = String::new();
    let mut rest = raw;

    while let Some(start) = rest.find("${") {
        rendered.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let end = after_start
            .find('}')
            .ok_or_else(|| anyhow!("unterminated runtime expression in string: {raw}"))?;
        rendered.push_str(&format_runtime_value(&eval_expression_value(
            &after_start[..end],
            context,
        )?));
        rest = &after_start[end + 1..];
    }

    rendered.push_str(rest);
    Ok(rendered)
}

fn format_runtime_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn set_context_path(context: &mut Value, path: &str, value: Value) -> Result<()> {
    let root = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context root must be an object"))?;
    let segments = path
        .trim()
        .trim_start_matches('.')
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return Err(anyhow!("set task path must not be empty"));
    }

    let (parents, leaf) = segments.split_at(segments.len() - 1);
    let mut current = root;
    for segment in parents {
        let entry = current
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(JsonMap::new()));
        if !entry.is_object() {
            return Err(anyhow!(
                "cannot assign nested workflow path through non-object segment '{}'",
                segment
            ));
        }
        current = entry
            .as_object_mut()
            .ok_or_else(|| anyhow!("workflow path segment '{}' is not an object", segment))?;
    }
    current.insert(leaf[0].to_string(), value);
    Ok(())
}

fn error_matches_filter(
    filter: &ErrorFilterDefinition,
    context: &Value,
    error_var: &str,
) -> Result<bool> {
    let Some(expected) = &filter.with else {
        return Ok(true);
    };

    for (key, value) in expected {
        let actual = extract_path_value(context, &format!(".{error_var}.{key}"))?;
        let expected = resolve_runtime_value(value, context)?;
        if actual != expected {
            return Ok(false);
        }
    }
    Ok(true)
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
