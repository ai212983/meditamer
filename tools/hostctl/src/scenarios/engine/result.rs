use crate::scenarios::engine::support::resolve_runtime_value;
use crate::scenarios::engine::support::set_context_path;
use anyhow::anyhow;
use anyhow::Result;
use serde_json::Map as JsonMap;
use serde_json::Value;
use serverless_workflow_core::models::task::TaskDefinitionFields;

pub(crate) fn bind_call_result(
    common: &TaskDefinitionFields,
    context: &mut Value,
    output: Option<Value>,
) -> Result<()> {
    let Some(output) = output else {
        return Ok(());
    };
    let Some(binding) = resolve_result_binding(common, context)? else {
        return Ok(());
    };

    if binding.merge {
        merge_call_result(context, binding.path.as_deref(), output)?;
    } else if let Some(path) = binding.path {
        set_context_path(context, &path, output)?;
    } else {
        return Err(anyhow!(
            "workflow call result binding requires either a path or merge=true"
        ));
    }

    Ok(())
}

fn resolve_result_binding(
    common: &TaskDefinitionFields,
    context: &Value,
) -> Result<Option<ResultBinding>> {
    let Some(hostctl) = common
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("hostctl"))
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };
    let Some(result) = hostctl.get("result") else {
        return Ok(None);
    };

    if let Some(path) = result.as_str() {
        return Ok(Some(ResultBinding {
            path: Some(path.to_string()),
            merge: false,
        }));
    }

    let Some(result) = result.as_object() else {
        return Err(anyhow!(
            "workflow hostctl.result metadata must be a string path or object"
        ));
    };

    let path = result
        .get("path")
        .map(|value| resolve_result_path(value, context))
        .transpose()?
        .flatten();
    let merge = result
        .get("merge")
        .map(|value| resolve_runtime_value(value, context))
        .transpose()?
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    Ok(Some(ResultBinding { path, merge }))
}

fn resolve_result_path(value: &Value, context: &Value) -> Result<Option<String>> {
    match value {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.starts_with("${") && trimmed.ends_with('}') {
                return Ok(resolve_runtime_value(value, context)?
                    .as_str()
                    .map(ToOwned::to_owned));
            }
            Ok(Some(raw.to_string()))
        }
        Value::Null => Ok(None),
        other => Ok(resolve_runtime_value(other, context)?
            .as_str()
            .map(ToOwned::to_owned)),
    }
}

pub(crate) fn merge_call_result(
    context: &mut Value,
    path: Option<&str>,
    output: Value,
) -> Result<()> {
    let Value::Object(output_map) = output else {
        return Err(anyhow!(
            "workflow call result merge requires an object output"
        ));
    };

    let target = if let Some(path) = path {
        ensure_context_object_path(context, path)?
    } else {
        context
            .as_object_mut()
            .ok_or_else(|| anyhow!("workflow context root must be an object"))?
    };

    for (key, value) in output_map {
        target.insert(key, value);
    }
    Ok(())
}

fn ensure_context_object_path<'a>(
    context: &'a mut Value,
    path: &str,
) -> Result<&'a mut JsonMap<String, Value>> {
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
        return Ok(root);
    }

    let mut current = root;
    for segment in segments {
        let entry = current
            .entry(segment.to_string())
            .or_insert_with(|| Value::Object(JsonMap::new()));
        if !entry.is_object() {
            return Err(anyhow!(
                "cannot merge workflow result through non-object segment '{}'",
                segment
            ));
        }
        current = entry
            .as_object_mut()
            .ok_or_else(|| anyhow!("workflow result segment '{}' is not an object", segment))?;
    }
    Ok(current)
}

pub(crate) struct ResultBinding {
    pub(crate) path: Option<String>,
    pub(crate) merge: bool,
}
