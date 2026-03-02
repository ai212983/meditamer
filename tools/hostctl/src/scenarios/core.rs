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
