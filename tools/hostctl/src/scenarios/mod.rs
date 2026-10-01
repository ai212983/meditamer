//! Serverless-workflow scenario runner.
//!
//! [`engine`] walks a workflow's task graph and executes it against a
//! [`WorkflowRuntime`]; [`conditions`] evaluates the switch and retry
//! expressions the graph branches on.

pub(crate) mod conditions;
pub(crate) mod engine;
#[cfg(test)]
mod tests;

use engine::execute_task_map;

use std::{fmt, fs, path::Path};

use anyhow::{Context, Result};
use serde_json::Value;
use serverless_workflow_core::models::workflow::WorkflowDefinition;

pub trait WorkflowRuntime {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()>;

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        self.invoke(action, args, context)?;
        Ok(None)
    }
}

#[derive(Debug)]
pub struct WorkflowActionError {
    message: String,
    context_patch: Option<Value>,
}

impl WorkflowActionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            context_patch: None,
        }
    }

    pub fn with_context_patch(mut self, patch: Value) -> Self {
        self.context_patch = Some(patch);
        self
    }

    pub fn context_patch(&self) -> Option<&Value> {
        self.context_patch.as_ref()
    }
}

impl fmt::Display for WorkflowActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for WorkflowActionError {}

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
