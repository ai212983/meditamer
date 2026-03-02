#[cfg(test)]
mod tests {
    use super::{eval_condition, execute_workflow, WorkflowRuntime};
    use anyhow::Result;
    use serde_json::Value;
    use serverless_workflow_core::models::workflow::WorkflowDefinition;

    struct TestRuntime {
        actions: Vec<String>,
    }

    impl WorkflowRuntime for TestRuntime {
        fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
            self.actions.push(action.to_string());
            if action == "inc" {
                let n = context.get("n").and_then(|v| v.as_i64()).unwrap_or(0) + 1;
                context["n"] = Value::from(n);
            }
            Ok(())
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
}
