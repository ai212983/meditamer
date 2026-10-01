use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::workflows::wifi::common::{parse_mem_diag_line, parse_scan_done_count, MemDiagKind};

use super::{
    probe::ProbeRoundState,
    profile::{recommended_round_timeout_ms, DiscoveryProfile},
};
use crate::{scenarios::load_workflow, workflows::wifi::common::NetPolicy};

#[test]
fn scan_done_parser_extracts_count() {
    assert_eq!(
        parse_scan_done_count("upload_http: event scan_done status=0 count=2 scan_id=42"),
        Some(2)
    );
    assert_eq!(
        parse_scan_done_count("upload_http: event scan_done status=0 count=0 scan_id=42"),
        Some(0)
    );
    assert_eq!(
        parse_scan_done_count("NET_STATUS {\"state\":\"Ready\"}"),
        None
    );
}

#[test]
fn mem_diag_parser_extracts_radio_sample() {
    let line = "upload_http: radio_mem stage=scan_active_before trigger=none feature=true state=Initialized total=4259840 used=110160 free=4149680 peak=110160 internal_free=59280 external_free=4090400 min_free=4149680 min_internal_free=59280 min_external_free=4090400";
    let sample = parse_mem_diag_line(line).expect("radio sample parses");
    assert_eq!(sample.kind, MemDiagKind::Radio);
    assert_eq!(sample.stage, "scan_active_before");
    assert_eq!(sample.internal_free, 59280);
    assert_eq!(sample.min_internal_free, 59280);
}

#[test]
fn zero_discovery_classification_detects_zero_only_round() {
    let mut state = ProbeRoundState::new(0, Instant::now() + Duration::from_secs(1));
    state.ingest_line(
        "upload_http: event scan_done status=0 count=0 scan_id=42",
        "scan ap ssid=test",
        true,
    );
    assert!(state.is_zero_discovery());
}

#[test]
fn probe_round_detects_ready_with_scan_and_ssid_visibility() {
    let mut state = ProbeRoundState::new(0, Instant::now() + Duration::from_secs(1));
    state.ingest_line(
        "upload_http: event scan_done status=0 count=2 scan_id=42",
        "scan ap ssid=test-ap",
        true,
    );
    state.ingest_line(
        "upload_http: scan ap ssid=test-ap rssi=-48 auth=WPA2",
        "scan ap ssid=test-ap",
        true,
    );
    state.ingest_line(
        "NET_STATUS {\"state\":\"Ready\",\"link\":true,\"ipv4\":\"192.168.1.9\",\"listener\":true,\"listener_enabled\":true}",
        "scan ap ssid=test-ap",
        true,
    );
    assert!(state.ready);
    assert_eq!(state.scan_nonzero_events, 1);
    assert_eq!(state.ssid_seen_events, 1);
    assert!(!state.is_zero_discovery());
}

#[test]
fn recommended_timeout_respects_lower_bound_and_profile_override() {
    let policy = NetPolicy::default();
    let mut profile = DiscoveryProfile {
        round_timeout_ms: 1_000,
        ..DiscoveryProfile::default()
    };
    let recommended = recommended_round_timeout_ms(&policy, &profile);
    assert!(recommended >= profile.round_timeout_ms as u64);
    assert!(recommended > policy.connect_timeout_ms as u64);

    profile.round_timeout_ms = 5_000_000;
    let overridden = recommended_round_timeout_ms(&policy, &profile);
    assert_eq!(overridden, profile.round_timeout_ms as u64);
}

#[test]
fn wifi_discovery_workflow_yaml_parses() {
    let workflow_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-discovery-debug.sw.yaml");
    load_workflow(&workflow_path).expect("wifi discovery workflow parses");
}

#[test]
fn discovery_provisions_only_after_acknowledged_storage_mode() {
    use crate::scenarios::{execute_workflow, WorkflowRuntime};
    use anyhow::{bail, Result};
    use serde_json::{json, Value};

    struct Runtime {
        commands: Vec<String>,
        reject_command: Option<&'static str>,
        provisioned: bool,
    }
    impl WorkflowRuntime for Runtime {
        fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
            match action {
                "state_command" => {
                    let command = args["command"].as_str().unwrap();
                    self.commands.push(command.into());
                    if self.reject_command == Some(command) {
                        bail!("state rejected");
                    }
                }
                "net_apply_config" => {
                    assert_eq!(
                        self.commands,
                        ["STATE SET upload=on", "STATE DIAG kind=NONE targets=NONE"]
                    );
                    self.provisioned = true;
                    // Stop before network rounds: this test covers the provisioning boundary.
                    bail!("provisioning reached");
                }
                "fail_captured" => bail!(
                    "{}",
                    context["discovery_error"]["message"].as_str().unwrap()
                ),
                _ => {}
            }
            Ok(())
        }
    }
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-discovery-debug.sw.yaml"),
    )
    .unwrap();
    for reject_command in [
        None,
        Some("STATE SET upload=on"),
        Some("STATE DIAG kind=NONE targets=NONE"),
    ] {
        let mut runtime = Runtime {
            commands: Vec::new(),
            reject_command,
            provisioned: false,
        };
        let error = execute_workflow(
            &workflow,
            &mut runtime,
            &json!({ "retain_reset_capture_secs": 0 }),
        )
        .unwrap_err();
        assert_eq!(runtime.provisioned, reject_command.is_none());
        assert!(format!("{error:#}").contains(if reject_command.is_none() {
            "provisioning reached"
        } else {
            "state rejected"
        }));
    }
}

#[test]
fn discovery_failure_runs_retained_capture_before_rethrow() {
    use crate::scenarios::{execute_workflow, WorkflowRuntime};
    use anyhow::{bail, Result};
    use serde_json::{json, Value};

    struct Runtime {
        captured: bool,
    }
    impl WorkflowRuntime for Runtime {
        fn invoke(&mut self, action: &str, _: &Value, context: &mut Value) -> Result<()> {
            match action {
                "net_apply_config" => bail!("diagnostic original failure"),
                "capture_reset_trace" => {
                    assert_eq!(context["retain_reset_capture_secs"], 120);
                    self.captured = true;
                }
                "fail_captured" => {
                    assert!(self.captured);
                    bail!(
                        "{}",
                        context["discovery_error"]["message"].as_str().unwrap()
                    );
                }
                _ => {}
            }
            Ok(())
        }
    }
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-discovery-debug.sw.yaml"),
    )
    .unwrap();
    let mut runtime = Runtime { captured: false };
    let error = execute_workflow(
        &workflow,
        &mut runtime,
        &json!({ "retain_reset_capture_secs": 120 }),
    )
    .unwrap_err();
    assert!(runtime.captured);
    assert!(error.to_string().contains("diagnostic original failure"));
}
