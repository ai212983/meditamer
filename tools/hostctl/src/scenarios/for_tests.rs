    #[test]
    fn repeat_loop_skips_body_when_count_is_zero() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "for-zero"
  version: "1.0.0"
do:
  - loop_task:
      do:
        - run_probe:
            call: "inc"
      metadata:
        hostctl:
          repeat:
            in: ".count"
            each: "probe"
            at: "probe_index"
  - done:
      call: "finish"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let context =
            execute_workflow(&workflow, &mut runtime, &serde_json::json!({ "count": 0, "n": 0 }))?;

        assert_eq!(context["n"].as_i64(), Some(0));
        assert_eq!(runtime.actions, vec!["finish"]);
        Ok(())
    }

    #[test]
    fn repeat_loop_runs_fixed_numeric_count() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "for-count"
  version: "1.0.0"
do:
  - loop_task:
      do:
        - capture:
            call: "capture_args"
            with:
              probe: ".probe"
              index: ".probe_index"
        - run_probe:
            call: "inc"
      metadata:
        hostctl:
          repeat:
            in: ".count"
            each: "probe"
            at: "probe_index"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let context =
            execute_workflow(&workflow, &mut runtime, &serde_json::json!({ "count": 3, "n": 0 }))?;

        assert_eq!(context["n"].as_i64(), Some(3));
        assert_eq!(context["captured_args"]["probe"].as_i64(), Some(2));
        assert_eq!(context["captured_args"]["index"].as_i64(), Some(2));
        Ok(())
    }

    #[test]
    fn repeat_loop_honors_while_condition() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "for-while"
  version: "1.0.0"
do:
  - loop_task:
      do:
        - run_probe:
            call: "inc"
      metadata:
        hostctl:
          repeat:
            in: ".count"
            each: "probe"
            at: "probe_index"
            while: ".probe_index < .limit"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let context = execute_workflow(
            &workflow,
            &mut runtime,
            &serde_json::json!({ "count": 5, "limit": 2, "n": 0 }),
        )?;

        assert_eq!(context["n"].as_i64(), Some(2));
        Ok(())
    }

    #[test]
    fn repeat_loop_allows_set_updates_inside_body() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "for-set"
  version: "1.0.0"
do:
  - loop_task:
      do:
        - remember:
            set:
              trace.last_probe: ".probe"
              trace.last_index: ".probe_index"
      metadata:
        hostctl:
          repeat:
            in: ".count"
            each: "probe"
            at: "probe_index"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let context =
            execute_workflow(&workflow, &mut runtime, &serde_json::json!({ "count": 2 }))?;

        assert_eq!(context["trace"]["last_probe"].as_i64(), Some(1));
        assert_eq!(context["trace"]["last_index"].as_i64(), Some(1));
        Ok(())
    }

    #[test]
    fn repeat_loop_supports_nested_loops() -> Result<()> {
        let yaml = r#"
document:
  dsl: "1.0.0"
  namespace: "hostctl"
  name: "for-nested"
  version: "1.0.0"
do:
  - outer_loop:
      do:
        - inner_loop:
            do:
              - run_probe:
                  call: "inc"
            metadata:
              hostctl:
                repeat:
                  in: ".inner_count"
                  each: "inner_probe"
                  at: "inner_index"
      metadata:
        hostctl:
          repeat:
            in: ".outer_count"
            each: "outer_probe"
            at: "outer_index"
"#;
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)?;
        let mut runtime = TestRuntime {
            actions: Vec::new(),
        };
        let context = execute_workflow(
            &workflow,
            &mut runtime,
            &serde_json::json!({ "outer_count": 2, "inner_count": 3, "n": 0 }),
        )?;

        assert_eq!(context["n"].as_i64(), Some(6));
        Ok(())
    }
