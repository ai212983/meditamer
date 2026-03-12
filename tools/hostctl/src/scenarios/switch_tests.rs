    #[test]
    fn switch_prefers_matching_branch_over_default() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "switch-match"
  version: "1.0.0"
do:
  - gate:
      switch:
        - pass:
            when: ".ok == true"
            then: "pass_task"
        - default:
            then: "fail_task"
  - fail_task:
      call: "fail"
  - pass_task:
      call: "finish"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        execute_workflow(&workflow, &mut runtime, &serde_json::json!({ "ok": true }))?;

        assert_eq!(runtime.actions, vec!["finish"]);
        Ok(())
    }

    #[test]
    fn switch_uses_default_when_no_case_matches() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "switch-default"
  version: "1.0.0"
do:
  - gate:
      switch:
        - fail:
            when: ".ok == false"
            then: "fail_task"
        - default:
            then: "pass_task"
  - fail_task:
      call: "fail"
  - pass_task:
      call: "finish"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        execute_workflow(&workflow, &mut runtime, &serde_json::json!({ "ok": true }))?;

        assert_eq!(runtime.actions, vec!["finish"]);
        Ok(())
    }

    #[test]
    fn switch_can_end_workflow_explicitly() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "switch-end"
  version: "1.0.0"
do:
  - gate:
      switch:
        - stop:
            when: "true"
            then: "__end__"
  - after:
      call: "finish"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        execute_workflow(&workflow, &mut runtime, &serde_json::json!({}))?;

        assert!(runtime.actions.is_empty());
        Ok(())
    }

    #[test]
    fn switch_uses_explicit_task_then_when_no_case_matches() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "switch-task-then"
  version: "1.0.0"
do:
  - gate:
      switch:
        - fail:
            when: ".ok == false"
            then: "fail_task"
      then: "after"
  - after:
      call: "finish"
      then: "__end__"
  - fail_task:
      call: "fail"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        execute_workflow(&workflow, &mut runtime, &serde_json::json!({ "ok": true }))?;

        assert_eq!(runtime.actions, vec!["finish"]);
        Ok(())
    }

    #[test]
    fn switch_errors_when_no_case_matches_and_no_default_exists() {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "switch-strict"
  version: "1.0.0"
do:
  - gate:
      switch:
        - fail:
            when: ".ok == false"
            then: "fail_task"
  - fail_task:
      call: "fail"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml).expect("workflow parses");
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let err = execute_workflow(&workflow, &mut runtime, &serde_json::json!({ "ok": true }))
            .expect_err("strict switch should reject implicit fallthrough");

        assert!(err
            .to_string()
            .contains("no matching case and no explicit default/then transition"));
    }
