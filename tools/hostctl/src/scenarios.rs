use std::{collections::HashMap, fs, path::Path};

use anyhow::{anyhow, Context, Result};
use serde_json::{Map as JsonMap, Value};
use serverless_workflow_core::models::{
    map::Map as WorkflowMap,
    task::{
        CallTaskDefinition, DoTaskDefinition, SwitchTaskDefinition, TaskDefinition,
        TaskDefinitionFields,
    },
    workflow::WorkflowDefinition,
};

pub trait WorkflowRuntime {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()>;
}

pub fn load_workflow(path: &Path) -> Result<WorkflowDefinition> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed reading workflow file {}", path.display()))?;
    serde_yaml::from_str::<WorkflowDefinition>(&raw)
        .with_context(|| format!("failed parsing workflow file {}", path.display()))
}

pub fn execute_workflow<R: WorkflowRuntime>(
    workflow: &WorkflowDefinition,
    runtime: &mut R,
    input: &Value,
) -> Result<Value> {
    let mut context = input.clone();
    execute_task_map(&workflow.do_, runtime, &mut context)?;
    Ok(context)
}

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

fn eval_condition(raw_condition: &str, context: &Value) -> Result<bool> {
    let condition = unwrap_expression(raw_condition);

    if condition.eq_ignore_ascii_case("true") {
        return Ok(true);
    }
    if condition.eq_ignore_ascii_case("false") {
        return Ok(false);
    }

    if let Some((left, right)) = condition.split_once("==") {
        let lhs = resolve_operand(context, left.trim())?;
        let rhs = resolve_operand(context, right.trim())?;
        return Ok(lhs == rhs);
    }
    if let Some((left, right)) = condition.split_once("!=") {
        let lhs = resolve_operand(context, left.trim())?;
        let rhs = resolve_operand(context, right.trim())?;
        return Ok(lhs != rhs);
    }
    if let Some((left, right)) = condition.split_once(">=") {
        return compare_numeric(context, left.trim(), right.trim(), |l, r| l >= r);
    }
    if let Some((left, right)) = condition.split_once("<=") {
        return compare_numeric(context, left.trim(), right.trim(), |l, r| l <= r);
    }
    if let Some((left, right)) = condition.split_once('>') {
        return compare_numeric(context, left.trim(), right.trim(), |l, r| l > r);
    }
    if let Some((left, right)) = condition.split_once('<') {
        return compare_numeric(context, left.trim(), right.trim(), |l, r| l < r);
    }

    Err(anyhow!("unsupported condition syntax: {raw_condition}"))
}

fn unwrap_expression(raw_condition: &str) -> &str {
    let condition = raw_condition.trim();
    if condition.starts_with("${") && condition.ends_with('}') && condition.len() >= 4 {
        condition[2..condition.len() - 1].trim()
    } else {
        condition
    }
}

fn extract_path_value(input: &Value, path: &str) -> Result<Value> {
    let trimmed = path.trim();
    let path = trimmed
        .strip_prefix('.')
        .ok_or_else(|| anyhow!("condition path must start with '.': {trimmed}"))?;

    let mut current = input;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        current = current
            .get(segment)
            .ok_or_else(|| anyhow!("missing input field in condition path: {segment}"))?;
    }
    Ok(current.clone())
}

fn parse_literal(raw: &str) -> Value {
    let raw = raw.trim();
    if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        return Value::String(raw[1..raw.len() - 1].to_string());
    }
    if raw.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if raw.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if let Ok(num) = raw.parse::<i64>() {
        return Value::Number(num.into());
    }
    if let Ok(num) = raw.parse::<f64>() {
        return serde_json::Number::from_f64(num)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(raw.to_string()));
    }
    Value::String(raw.to_string())
}

fn resolve_operand(context: &Value, raw: &str) -> Result<Value> {
    let trimmed = raw.trim();
    if trimmed.starts_with('.') {
        extract_path_value(context, trimmed)
    } else {
        Ok(parse_literal(trimmed))
    }
}

fn compare_numeric<F>(context: &Value, left: &str, right: &str, cmp: F) -> Result<bool>
where
    F: Fn(f64, f64) -> bool,
{
    let lhs = resolve_operand(context, left)?;
    let rhs = resolve_operand(context, right)?;

    let l = lhs
        .as_f64()
        .ok_or_else(|| anyhow!("left side is not numeric: {left}"))?;
    let r = rhs
        .as_f64()
        .ok_or_else(|| anyhow!("right side is not numeric: {right}"))?;
    Ok(cmp(l, r))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serverless_workflow_core::models::workflow::WorkflowDefinition;

    struct TestRuntime {
        actions: Vec<String>,
    }

    impl WorkflowRuntime for TestRuntime {
        fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
            self.actions.push(action.to_string());
            if action == "inc" {
                let n = context.get("n").and_then(|v| v.as_i64()).unwrap_or(0) + 1;
                context["n"] = Value::from(n);
            }
            Ok(())
        }
    }

    #[test]
    fn executes_switch_loop_with_mutable_context() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "loop"
  version: "1.0.0"
do:
  - inc_task:
      call: "inc"
      then: "gate"
  - gate:
      switch:
        - again:
            when: ".n < 3"
            then: "inc_task"
        - done:
            then: "finish"
  - finish:
      call: "finish"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let context = execute_workflow(&workflow, &mut runtime, &serde_json::json!({ "n": 0 }))?;

        assert_eq!(context["n"].as_i64(), Some(3));
        assert_eq!(
            runtime.actions,
            vec![
                "inc".to_string(),
                "inc".to_string(),
                "inc".to_string(),
                "finish".to_string()
            ]
        );
        Ok(())
    }

    #[test]
    fn supports_numeric_comparators() -> Result<()> {
        assert!(eval_condition(
            ".value >= 10",
            &serde_json::json!({ "value": 10 })
        )?);
        assert!(eval_condition(
            ".value < 11",
            &serde_json::json!({ "value": 10 })
        )?);
        assert!(!eval_condition(
            ".value > 10",
            &serde_json::json!({ "value": 10 })
        )?);
        assert!(eval_condition(
            ".value <= .limit",
            &serde_json::json!({ "value": 10, "limit": 12 })
        )?);
        assert!(!eval_condition(
            ".value >= .limit",
            &serde_json::json!({ "value": 10, "limit": 12 })
        )?);
        Ok(())
    }

    #[test]
    fn supports_equality_between_context_paths() -> Result<()> {
        assert!(eval_condition(
            ".lhs == .rhs",
            &serde_json::json!({ "lhs": "ok", "rhs": "ok" })
        )?);
        assert!(eval_condition(
            ".lhs != .rhs",
            &serde_json::json!({ "lhs": 1, "rhs": 2 })
        )?);
        Ok(())
    }
}
