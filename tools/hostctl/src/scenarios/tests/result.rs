use super::TestRuntime;
use crate::scenarios::execute_workflow;
use anyhow::Result;
use serverless_workflow_core::models::workflow::WorkflowDefinition;

#[test]
fn call_result_binding_replaces_scalar_path() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "result-scalar"
  version: "1.0.0"
do:
  - fetch:
      call: "return_scalar"
      metadata:
        hostctl:
          result: ".result.scalar"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(&workflow, &mut runtime, &serde_json::json!({}))?;

    assert_eq!(context["result"]["scalar"].as_i64(), Some(7));
    Ok(())
}

#[test]
fn call_result_binding_replaces_object_path() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "result-object"
  version: "1.0.0"
do:
  - fetch:
      call: "return_object"
      with:
        value: "boot"
      metadata:
        hostctl:
          result:
            path: ".probe"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(&workflow, &mut runtime, &serde_json::json!({}))?;

    assert_eq!(context["probe"]["status"].as_str(), Some("ok"));
    assert_eq!(context["probe"]["echo"].as_str(), Some("boot"));
    Ok(())
}

#[test]
fn call_result_binding_supports_nested_path_resolution() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "result-nested"
  version: "1.0.0"
do:
  - fetch:
      call: "return_object"
      with:
        value: ".mode"
      metadata:
        hostctl:
          result:
            path: ".captures.latest"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({ "mode": "stream" }),
    )?;

    assert_eq!(
        context["captures"]["latest"]["echo"].as_str(),
        Some("stream")
    );
    Ok(())
}

#[test]
fn call_result_binding_supports_merge_behavior() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "result-merge"
  version: "1.0.0"
do:
  - seed:
      set:
        probe.prev: "keep"
  - fetch:
      call: "return_object"
      with:
        value: 42
      metadata:
        hostctl:
          result:
            path: ".probe"
            merge: true
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(&workflow, &mut runtime, &serde_json::json!({}))?;

    assert_eq!(context["probe"]["prev"].as_str(), Some("keep"));
    assert_eq!(context["probe"]["status"].as_str(), Some("ok"));
    assert_eq!(context["probe"]["echo"].as_i64(), Some(42));
    Ok(())
}
