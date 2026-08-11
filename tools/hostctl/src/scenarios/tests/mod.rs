mod call_retry;
mod error_patch;
mod expression;
mod for_each;
mod result;
mod switch;
mod try_retry;
mod workflow_contract;

use crate::scenarios::conditions::eval_condition;
use crate::scenarios::{execute_workflow, WorkflowActionError, WorkflowRuntime};

use anyhow::Result;
use serde_json::Value;
use serverless_workflow_core::models::workflow::WorkflowDefinition;

pub(super) struct TestRuntime {
    pub(super) actions: Vec<String>,
}

impl WorkflowRuntime for TestRuntime {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        self.actions.push(action.to_string());
        if action == "inc" {
            let n = context.get("n").and_then(|v| v.as_i64()).unwrap_or(0) + 1;
            context["n"] = Value::from(n);
        }
        if action == "flaky" {
            let remaining = context
                .get("remaining_failures")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if remaining > 0 {
                context["remaining_failures"] = Value::from(remaining - 1);
                anyhow::bail!("retryable");
            }
        }
        if action == "non_retryable" {
            anyhow::bail!("fatal");
        }
        if action == "capture_args" {
            context["captured_args"] = args.clone();
        }
        if action == "fail" {
            anyhow::bail!("boom");
        }
        if action == "fail_with_patch" {
            return Err(WorkflowActionError::new("boom")
                .with_context_patch(serde_json::json!({
                    "failure_class": "uart_transport",
                    "flash_ok": false
                }))
                .into());
        }
        if action == "flaky_with_patch" {
            let remaining = context
                .get("remaining_failures")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if remaining > 0 {
                context["remaining_failures"] = Value::from(remaining - 1);
                return Err(WorkflowActionError::new("boom")
                    .with_context_patch(serde_json::json!({
                        "failure_class": "uart_transport",
                        "flash_ok": false
                    }))
                    .into());
            }
        }
        Ok(())
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        match action {
            "return_scalar" => {
                self.actions.push(action.to_string());
                Ok(Some(Value::from(7)))
            }
            "return_object" => {
                self.actions.push(action.to_string());
                Ok(Some(serde_json::json!({
                    "status": "ok",
                    "echo": args.get("value").cloned().unwrap_or(Value::Null),
                })))
            }
            _ => {
                self.invoke(action, args, context)?;
                Ok(None)
            }
        }
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

#[test]
fn call_with_resolves_context_paths() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "call-with"
  version: "1.0.0"
do:
  - capture:
      call: "capture_args"
      with:
        mode: "boot"
        port: ".port"
        nested:
          baud: ".baud"
          enabled: "${ true }"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({ "port": "/dev/cu.usbserial-510", "baud": 115200 }),
    )?;

    assert_eq!(context["captured_args"]["mode"].as_str(), Some("boot"));
    assert_eq!(
        context["captured_args"]["port"].as_str(),
        Some("/dev/cu.usbserial-510")
    );
    assert_eq!(
        context["captured_args"]["nested"]["baud"].as_i64(),
        Some(115200)
    );
    assert_eq!(
        context["captured_args"]["nested"]["enabled"].as_bool(),
        Some(true)
    );
    Ok(())
}

#[test]
fn set_task_updates_nested_context_from_expressions() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "set-values"
  version: "1.0.0"
do:
  - seed:
      set:
        flags.flash_ok: false
        flags.capture_mode: ".capture_mode"
        result.bytes: "${ .capture_bytes }"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({ "capture_mode": "boot", "capture_bytes": 42 }),
    )?;

    assert_eq!(context["flags"]["flash_ok"].as_bool(), Some(false));
    assert_eq!(context["flags"]["capture_mode"].as_str(), Some("boot"));
    assert_eq!(context["result"]["bytes"].as_i64(), Some(42));
    Ok(())
}

#[test]
fn try_task_catches_error_and_runs_handler() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "try-catch"
  version: "1.0.0"
do:
  - guarded:
      try:
        - attempt:
            call: "fail"
      catch:
        as: "flash_error"
        do:
          - mark:
              set:
                flash_ok: false
                flash_error_text: ".flash_error.message"
  - done:
      call: "finish"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(&workflow, &mut runtime, &serde_json::json!({}))?;

    assert_eq!(context["flash_ok"].as_bool(), Some(false));
    assert_eq!(context["flash_error"]["message"].as_str(), Some("boom"));
    assert_eq!(context["flash_error_text"].as_str(), Some("boom"));
    assert_eq!(
        runtime.actions,
        vec!["fail".to_string(), "finish".to_string()]
    );
    Ok(())
}
