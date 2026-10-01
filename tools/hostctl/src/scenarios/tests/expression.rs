use super::TestRuntime;
use crate::scenarios::conditions::eval_condition;
use crate::scenarios::execute_workflow;
use anyhow::Result;
use serverless_workflow_core::models::workflow::WorkflowDefinition;

#[test]
fn supports_boolean_operators_with_precedence_and_grouping() -> Result<()> {
    let context = serde_json::json!({
        "ready": true,
        "retryable": true,
        "attempt": 1,
        "limit": 2
    });

    assert!(eval_condition(
        ".ready == true && .retryable == true && .attempt < .limit",
        &context
    )?);
    assert!(eval_condition(
        "!(.ready == false || .attempt >= .limit)",
        &context
    )?);
    assert!(!eval_condition(
        ".ready == false || (.retryable == false && .attempt < .limit)",
        &context
    )?);
    Ok(())
}

#[test]
fn supports_null_and_presence_checks() -> Result<()> {
    let context = serde_json::json!({
        "present_value": 7,
        "nullable": null
    });

    assert!(eval_condition("exists(.present_value)", &context)?);
    assert!(!eval_condition("exists(.missing_value)", &context)?);
    assert!(eval_condition("present(.present_value)", &context)?);
    assert!(!eval_condition("present(.nullable)", &context)?);
    assert!(eval_condition(".nullable == null", &context)?);
    Ok(())
}

#[test]
fn set_task_supports_arithmetic_expressions() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "set-arithmetic"
  version: "1.0.0"
do:
  - advance:
      set:
        cycle: "${ .cycle + 1 }"
        remaining: "${ .remaining - 2 }"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({ "cycle": 1, "remaining": 5 }),
    )?;

    assert_eq!(context["cycle"].as_i64(), Some(2));
    assert_eq!(context["remaining"].as_i64(), Some(3));
    Ok(())
}

#[test]
fn call_with_supports_string_interpolation() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "call-with-interpolation"
  version: "1.0.0"
do:
  - capture:
      call: "capture_args"
      with:
        summary: "retry ${ .attempt + 1 }/${ .limit } on ${ .port }"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({
            "attempt": 1,
            "limit": 3,
            "port": "/dev/cu.usbserial-510"
        }),
    )?;

    assert_eq!(
        context["captured_args"]["summary"].as_str(),
        Some("retry 2/3 on /dev/cu.usbserial-510")
    );
    Ok(())
}
