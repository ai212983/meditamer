#[test]
fn call_task_retries_until_success() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "call-retry-success"
  version: "1.0.0"
do:
  - guarded:
      call: "flaky"
      metadata:
        hostctl:
          retry:
            count: 2
            delayMs: 0
            as: "call_error"
  - done:
      call: "finish"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({ "remaining_failures": 2 }),
    )?;

    assert_eq!(context["remaining_failures"].as_i64(), Some(0));
    assert_eq!(runtime.actions, vec!["flaky", "flaky", "flaky", "finish"]);
    Ok(())
}

#[test]
fn call_task_exposes_bound_error_after_retry_exhaustion() {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "call-retry-fail"
  version: "1.0.0"
do:
  - guarded:
      call: "non_retryable"
      metadata:
        hostctl:
          retry:
            count: 1
            delayMs: 0
            as: "call_error"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml).expect("workflow parses");
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let err = execute_workflow(&workflow, &mut runtime, &serde_json::json!({}))
        .expect_err("workflow should fail after retry exhaustion");

    assert_eq!(err.to_string(), "fatal");
    assert_eq!(runtime.actions, vec!["non_retryable", "non_retryable"]);
}

#[test]
fn call_task_retry_honors_metadata_conditions() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "call-retry-conditional"
  version: "1.0.0"
do:
  - guarded:
      call: "non_retryable"
      metadata:
        hostctl:
          retry:
            count: 3
            delayMs: 0
            as: "call_error"
            when: "${ .call_error.message == \"retryable\" }"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let err = execute_workflow(&workflow, &mut runtime, &serde_json::json!({}))
        .expect_err("workflow should fail without retry");

    assert_eq!(err.to_string(), "fatal");
    assert_eq!(runtime.actions, vec!["non_retryable"]);
    Ok(())
}

#[test]
fn call_task_retry_reads_count_delay_and_conditions_from_context() -> Result<()> {
    let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "call-retry-metadata"
  version: "1.0.0"
do:
  - guarded:
      call: "flaky"
      metadata:
        hostctl:
          retry:
            count: ".retry_count"
            delayMs: ".retry_delay_ms"
            as: "call_error"
            when: ".should_retry == true"
  - done:
      call: "finish"
"#;
    let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
    let mut runtime = TestRuntime {
        actions: Vec::new(),
    };
    let context = execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({
            "remaining_failures": 1,
            "retry_count": 1,
            "retry_delay_ms": 0,
            "should_retry": true
        }),
    )?;

    assert_eq!(context["remaining_failures"].as_i64(), Some(0));
    assert_eq!(runtime.actions, vec!["flaky", "flaky", "finish"]);
    Ok(())
}
