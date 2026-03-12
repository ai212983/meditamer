    #[test]
    fn try_task_merges_error_context_patch_before_catch_flow() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "try-error-patch"
  version: "1.0.0"
do:
  - guarded:
      try:
        - attempt:
            call: "fail_with_patch"
      catch:
        as: "flash_error"
      then: "gate"
  - gate:
      switch:
        - failed:
            when: ".flash_ok == false && .failure_class == \"uart_transport\""
            then: "__end__"
        - default:
            then: "fail_task"
  - fail_task:
      call: "fail"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let context = execute_workflow(&workflow, &mut runtime, &serde_json::json!({}))?;

        assert_eq!(context["flash_ok"].as_bool(), Some(false));
        assert_eq!(context["failure_class"].as_str(), Some("uart_transport"));
        assert_eq!(context["flash_error"]["message"].as_str(), Some("boom"));
        Ok(())
    }

    #[test]
    fn call_retry_conditions_can_use_error_context_patch() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "call-error-patch"
  version: "1.0.0"
do:
  - attempt:
      call: "flaky_with_patch"
      metadata:
        hostctl:
          retry:
            count: 1
            when: ".failure_class == \"uart_transport\""
      then: "finish"
  - finish:
      call: "capture_args"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let context = execute_workflow(
            &workflow,
            &mut runtime,
            &serde_json::json!({ "remaining_failures": 1 }),
        )?;

        assert_eq!(context["remaining_failures"].as_i64(), Some(0));
        assert_eq!(
            runtime.actions,
            vec![
                "flaky_with_patch".to_string(),
                "flaky_with_patch".to_string(),
                "capture_args".to_string()
            ]
        );
        Ok(())
    }
