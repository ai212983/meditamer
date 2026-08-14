use anyhow::Result;

#[test]
fn checked_in_scenarios_require_explicit_switch_defaults() -> Result<()> {
    use std::fs;
    use std::path::PathBuf;

    fn assert_switch_defaults(node: &serde_yaml::Value, path: &str) {
        match node {
            serde_yaml::Value::Mapping(map) => {
                for (key, value) in map {
                    if key.as_str() == Some("switch") {
                        let entries = value
                            .as_sequence()
                            .expect("workflow switch entries should be a sequence");
                        let has_default = entries.iter().any(|entry| {
                            entry
                                .as_mapping()
                                .is_some_and(|entry_map| entry_map.contains_key("default"))
                        });
                        assert!(
                            has_default,
                            "workflow switch in {path} is missing an explicit default branch"
                        );
                    }
                    assert_switch_defaults(value, path);
                }
            }
            serde_yaml::Value::Sequence(items) => {
                for item in items {
                    assert_switch_defaults(item, path);
                }
            }
            _ => {}
        }
    }

    let scenarios_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios");
    let mut paths = fs::read_dir(&scenarios_dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("yaml"))
        .collect::<Vec<_>>();
    paths.sort();

    for path in paths {
        let raw = fs::read_to_string(&path)?;
        let yaml: serde_yaml::Value = serde_yaml::from_str(&raw)?;
        assert_switch_defaults(&yaml, &path.display().to_string());
    }

    Ok(())
}
