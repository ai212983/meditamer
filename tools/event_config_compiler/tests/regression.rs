use std::{fs, path::PathBuf};

use event_config_compiler::{
    generate_from_path, parse_events_file, render_generated_config, validate_config,
    ConfigCompilerError,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("missing tools dir")
        .parent()
        .expect("missing repo root")
        .to_path_buf()
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn snapshot(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(name)
}

#[test]
fn default_config_matches_snapshot() {
    let config = repo_root().join("config/events.toml");
    let actual = generate_from_path(&config).expect("default config should compile");
    let expected = fs::read_to_string(snapshot("default_generated.rs"))
        .expect("missing default snapshot file");

    assert_eq!(
        actual, expected,
        "generated output changed; if intentional, update tools/event_config_compiler/tests/snapshots/default_generated.rs"
    );
}

#[test]
fn generation_is_deterministic_for_same_input() {
    let config = fixture("valid_default.toml");
    let first = generate_from_path(&config).expect("first generation failed");
    let second = generate_from_path(&config).expect("second generation failed");
    assert_eq!(first, second);
}

#[test]
fn optional_events_flags_render_when_enabled() {
    let path = fixture("valid_optional_enabled.toml");
    let events = parse_events_file(&path).expect("fixture should parse");
    validate_config(&events).expect("fixture should validate");
    let rendered = render_generated_config(&events);

    for needle in [
        "pickup_enabled: true",
        "placement_enabled: true",
        "stillness_start_enabled: true",
        "stillness_end_enabled: true",
        "near_intent_enabled: true",
        "far_intent_enabled: true",
    ] {
        assert!(
            rendered.contains(needle),
            "rendered output missing `{needle}`"
        );
    }
}

#[test]
fn semantic_validation_rejects_invalid_ranges_and_weights() {
    let cases = [
        (
            "invalid/min_gap_zero.toml",
            "triple_tap.min_gap_ms must be > 0",
        ),
        (
            "invalid/max_lt_min.toml",
            "triple_tap.max_gap_ms must be >= triple_tap.min_gap_ms",
        ),
        (
            "invalid/last_lt_max.toml",
            "triple_tap.last_max_gap_ms must be >= triple_tap.max_gap_ms",
        ),
        (
            "invalid/nonpositive_threshold.toml",
            "all triple_tap.thresholds fields must be positive integers",
        ),
        (
            "invalid/zero_weights.toml",
            "triple_tap.weights must contain at least one non-zero weight",
        ),
    ];

    for (fixture_name, expected_msg) in cases {
        let path = fixture(fixture_name);
        let err = generate_from_path(&path).expect_err("fixture should fail validation");
        match err {
            ConfigCompilerError::Validation(msg) => {
                assert!(
                    msg.contains(expected_msg),
                    "expected validation message containing `{expected_msg}`, got `{msg}`"
                );
            }
            other => panic!("expected validation error, got {other}"),
        }
    }
}

#[test]
fn parse_errors_are_reported_for_schema_mismatches() {
    let path = fixture("invalid/missing_optional_events.toml");
    let err = generate_from_path(&path).expect_err("fixture should fail parsing");

    match err {
        ConfigCompilerError::Parse(msg) => {
            assert!(
                msg.contains("optional_events"),
                "expected parse error mentioning optional_events, got `{msg}`"
            );
        }
        other => panic!("expected parse error, got {other}"),
    }
}
