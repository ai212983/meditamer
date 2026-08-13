use super::{analysis::analyze_lines, sha256, validate_artifacts};
use crate::scenarios::load_workflow;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

fn sample(stage: &str, cycle: u8, internal: u64) -> String {
    format!(
        "BLE_PHASE1D sample stage={stage} cycle={cycle} coex=true wifi_controller=true net_runner=true wifi_link=true dhcp=true listener=true internal_free={internal} internal_min=18000 cpu0_stack_min=9000 touch_stack_min=1200 callback_admission=true callback_in_flight=0 callback_accepted=3 callback_rejected=0 callback_high_water=1 rx_queue_high_water=1 rx_queue_overflow=0 rx_oversize=0 tx_rejected=0 tx_timeout=0 transport_faulted=false packets_free=4 pool_exhausted=0 wifi_ok=true resource_ok=true"
    )
}

fn close(cycle: u8) -> String {
    format!(
        "BLE_PHASE1D close cycle={cycle} deadline_ms=2000 pre_in_flight=0 accepted=3 rejected=0 callback_high_water=1 settled_in_flight=0 rx_queue_high_water=1 rx_queue_overflow=0 rx_oversize=0 tx_rejected=0 tx_timeout=0 transport_faulted=false packets_free=4 pool_exhausted=0"
    )
}

fn passing_lines() -> Vec<String> {
    let mut lines = vec![sample("before", 0, 24_000)];
    for cycle in 1..=20 {
        lines.push(sample("active", cycle, 18_000));
        lines.push(close(cycle));
        lines.push(sample("after", cycle, 23_500));
    }
    lines.push("BLE_PHASE1D state=completed cycle=20 failure=none".to_owned());
    lines
}

#[test]
fn complete_baseline_passes_but_full_phase1d_remains_open() {
    let report = analyze_lines(&passing_lines());
    assert!(report.baseline_passed, "{:?}", report.violations);
    assert!(!report.phase1d_gate_passed);
    assert_eq!(report.closed_cycles, 20);
    assert_eq!(report.fault_latched_close_cycles, 0);
    assert_eq!(report.post_warmup_internal_drift, Some(0));
    assert_eq!(report.opaque_internal_allocation_upper_bound, Some(6_000));
    assert_eq!(report.remaining_gates.len(), 3);
}

#[test]
fn missing_cycle_and_resource_floor_fail_baseline() {
    let mut lines = passing_lines();
    lines.retain(|line| !line.contains("stage=active cycle=7"));
    lines.push(sample("active", 7, 8_000));
    let report = analyze_lines(&lines);
    assert!(!report.baseline_passed);
    assert!(report
        .violations
        .iter()
        .any(|violation| violation.contains("internal_free below")));
}

#[test]
fn late_or_leaked_close_fails_baseline() {
    let mut lines = passing_lines();
    let bad = lines
        .iter_mut()
        .find(|line| line.starts_with("BLE_PHASE1D close cycle=4 "))
        .expect("cycle 4 close");
    *bad = bad
        .replace("settled_in_flight=0", "settled_in_flight=1")
        .replace("packets_free=4", "packets_free=3");
    let report = analyze_lines(&lines);
    assert!(!report.baseline_passed);
    assert!(report.violations.len() >= 2);
}

#[test]
fn workflow_yaml_parses_and_keeps_the_evidence_gate() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1d.sw.yaml");
    let workflow = load_workflow(&path).expect("workflow loads");
    assert_eq!(workflow.document.name, "ble-phase1d");
    let raw = std::fs::read_to_string(path).expect("workflow source");
    assert!(raw.contains("baseline_passed == true"));
    assert!(raw.contains("call: \"fail_evidence\""));
}

fn write_artifacts(temp: &TempDir, dirty: &str) {
    let elf = temp.path().join("firmware.elf");
    let app = temp.path().join("app.bin");
    fs::write(&elf, b"elf").expect("elf");
    fs::write(&app, b"app").expect("app");
    fs::write(
        temp.path().join("sha256.txt"),
        format!(
            "{}  firmware.elf\n{}  app.bin\n",
            sha256(&elf).expect("elf digest"),
            sha256(&app).expect("app digest")
        ),
    )
    .expect("hashes");
    fs::write(
        temp.path().join("build-metadata.txt"),
        format!(
            "profile=ble-release\nfeatures=ble-foundation\nfirmware_build_id=ble-p1d-test\ngit_head=0123456789abcdef0123456789abcdef01234567\ngit_status_begin\n{dirty}\ngit_status_end\n"
        ),
    )
    .expect("metadata");
}

#[test]
fn artifact_identity_requires_matching_hashes_and_clean_source() {
    let temp = TempDir::new().expect("tempdir");
    write_artifacts(&temp, "");
    let identity = validate_artifacts(temp.path()).expect("valid artifacts");
    assert_eq!(identity.build_id, "ble-p1d-test");
}

#[test]
fn artifact_identity_rejects_dirty_source() {
    let temp = TempDir::new().expect("tempdir");
    write_artifacts(&temp, " M src/firmware/ble/mod.rs\n");
    let error = validate_artifacts(temp.path()).expect_err("dirty source must fail");
    assert!(error.to_string().contains("dirty"));
}
