const MAX_REPEAT_ITEMS: usize = 4096;

fn execute_repeatable_do_task<R: WorkflowRuntime>(
    task: &DoTaskDefinition,
    runtime: &mut R,
    context: &mut Value,
) -> Result<bool> {
    let Some(repeat) = resolve_repeat_spec(&task.common, context)? else {
        return Ok(false);
    };

    for (index, item) in repeat.items.into_iter().enumerate() {
        if let Some(each) = &repeat.each {
            set_context_path(context, each, item)?;
        }
        if let Some(at) = &repeat.at {
            set_context_path(context, at, Value::from(index as u64))?;
        }
        if let Some(condition) = &repeat.while_ {
            if !eval_condition(condition, context)? {
                break;
            }
        }
        execute_task_map(&task.do_, runtime, context)?;
    }

    Ok(true)
}

fn resolve_repeat_spec(common: &TaskDefinitionFields, context: &Value) -> Result<Option<RepeatSpec>> {
    let Some(hostctl) = common
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("hostctl"))
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };
    let Some(repeat) = hostctl.get("repeat").and_then(Value::as_object) else {
        return Ok(None);
    };

    let items = resolve_repeat_items(
        repeat
            .get("in")
            .ok_or_else(|| anyhow!("workflow repeat metadata requires 'in'"))?,
        context,
    )?;

    Ok(Some(RepeatSpec {
        items,
        each: repeat
            .get("each")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        at: repeat
            .get("at")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        while_: repeat
            .get("while")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    }))
}

fn resolve_repeat_items(value: &Value, context: &Value) -> Result<Vec<Value>> {
    let resolved = resolve_runtime_value(value, context)?;
    match resolved {
        Value::Array(items) => {
            if items.len() > MAX_REPEAT_ITEMS {
                return Err(anyhow!(
                    "workflow repeat exceeds maximum item count: {} > {}",
                    items.len(),
                    MAX_REPEAT_ITEMS
                ));
            }
            Ok(items)
        }
        Value::Number(number) => {
            let count = number
                .as_u64()
                .ok_or_else(|| anyhow!("workflow repeat count must be a non-negative integer"))?;
            if count as usize > MAX_REPEAT_ITEMS {
                return Err(anyhow!(
                    "workflow repeat exceeds maximum item count: {} > {}",
                    count,
                    MAX_REPEAT_ITEMS
                ));
            }
            Ok((0..count).map(Value::from).collect())
        }
        other => Err(anyhow!(
            "workflow repeat 'in' must resolve to an array or non-negative integer, got {}",
            json_type_name(&other)
        )),
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

struct RepeatSpec {
    items: Vec<Value>,
    each: Option<String>,
    at: Option<String>,
    while_: Option<String>,
}
