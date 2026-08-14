use std::time::Duration;

use super::{BootDiscoveryGateConfig, BootDiscoveryLoopState};

fn cfg(allow_ready_only_fallback: bool) -> BootDiscoveryGateConfig {
    BootDiscoveryGateConfig {
        max_boot_uptime_ms: 30_000,
        timeout_ms: 5_000,
        settle_ms: 6_000,
        allow_ready_only_fallback,
    }
}

#[test]
fn boot_gate_passes_with_ready_scan_and_ssid_evidence() {
    let mut state = BootDiscoveryLoopState::new(cfg(false));
    state.observe_boot_line(
        "upload_http: event scan_done status=0 count=2 scan_id=1",
        "test-ap",
    );
    state.observe_boot_line("upload_http: scan ap ssid=test-ap rssi=-50", "test-ap");
    state.observe_boot_line(
        "NET_STATUS {\"state\":\"Ready\",\"link\":true,\"ipv4\":\"192.168.1.8\",\"listener\":false,\"listener_enabled\":false}",
        "test-ap",
    );
    assert!(state.reached_success());
}

#[test]
fn boot_gate_fails_without_scan_and_ssid_evidence() {
    let mut state = BootDiscoveryLoopState::new(cfg(false));
    state.observe_boot_line(
        "NET_STATUS {\"state\":\"Ready\",\"link\":true,\"ipv4\":\"192.168.1.8\",\"listener\":false,\"listener_enabled\":false}",
        "test-ap",
    );
    assert!(!state.reached_success());
    assert!(!state.should_fallback(&cfg(false)));
}

#[test]
fn ready_only_fallback_requires_explicit_enable() {
    let mut state = BootDiscoveryLoopState::new(cfg(true));
    state.ready = true;
    state.ready_only_fallback_after = Duration::from_millis(0);
    assert!(state.should_fallback(&cfg(true)));
    assert!(!state.should_fallback(&cfg(false)));
}

#[test]
fn ready_ssid_fallback_allows_missing_scan_count() {
    let mut state = BootDiscoveryLoopState::new(cfg(false));
    state.ready = true;
    state.ssid_seen_events = 2;
    state.scan_nonzero_events = 0;
    state.ready_ssid_fallback_after = Duration::from_millis(0);
    assert!(state.reached_ready_ssid_fallback());
}
