    #[test]
    fn try_task_retries_until_success() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "try-retry-success"
  version: "1.0.0"
do:
  - guarded:
      try:
        - attempt:
            call: "flaky"
      catch:
        as: "flash_error"
        retry:
          limit:
            attempt:
              count: 2
          delay:
            milliseconds: 0
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
    fn try_task_runs_catch_after_retry_exhaustion() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "try-retry-fail"
  version: "1.0.0"
do:
  - guarded:
      try:
        - attempt:
            call: "non_retryable"
      catch:
        as: "flash_error"
        retry:
          limit:
            attempt:
              count: 1
          delay:
            milliseconds: 0
        do:
          - mark:
              set:
                handled: true
                handled_message: ".flash_error.message"
  - done:
      call: "finish"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let context = execute_workflow(&workflow, &mut runtime, &serde_json::json!({}))?;

        assert_eq!(context["handled"].as_bool(), Some(true));
        assert_eq!(context["handled_message"].as_str(), Some("fatal"));
        assert_eq!(context["flash_error"]["retry_attempt"].as_u64(), Some(1));
        assert_eq!(runtime.actions, vec!["non_retryable", "non_retryable", "finish"]);
        Ok(())
    }

    #[test]
    fn try_task_skips_retry_when_condition_is_false() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "try-retry-conditional"
  version: "1.0.0"
do:
  - guarded:
      try:
        - attempt:
            call: "non_retryable"
      catch:
        as: "flash_error"
        retry:
          when: "${ .flash_error.message == \"retryable\" }"
          limit:
            attempt:
              count: 3
          delay:
            milliseconds: 0
        do:
          - mark:
              set:
                retries_skipped: true
  - done:
      call: "finish"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let context = execute_workflow(&workflow, &mut runtime, &serde_json::json!({}))?;

        assert_eq!(context["retries_skipped"].as_bool(), Some(true));
        assert_eq!(runtime.actions, vec!["non_retryable", "finish"]);
        Ok(())
    }

    #[test]
    fn retry_metadata_resolves_count_and_delay_from_context() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "try-retry-metadata"
  version: "1.0.0"
do:
  - guarded:
      try:
        - attempt:
            call: "flaky"
      catch:
        as: "flash_error"
        retry:
          limit:
            attempt:
              count: 0
      metadata:
        hostctl:
          retry:
            count: ".retry_count"
            delayMs: ".retry_delay_ms"
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
                "retry_delay_ms": 0
            }),
        )?;

        assert_eq!(context["remaining_failures"].as_i64(), Some(0));
        assert_eq!(runtime.actions, vec!["flaky", "flaky", "finish"]);
        Ok(())
    }
