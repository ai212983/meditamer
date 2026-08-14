use super::health::{
    format_health_status_error, is_ready_without_listener, should_force_recover_before_start,
    should_retry_wait_ready_after_recover,
};
use super::start::{metric_bool, metric_u32, parse_serving_allocator_status};
use crate::workflows::wifi::common::NetStatus;
use reqwest::StatusCode;

fn net_status(state: Option<&str>) -> NetStatus {
    NetStatus {
        state: state.map(ToOwned::to_owned),
        link: None,
        ipv4: None,
        listener: None,
        listener_enabled: None,
        failure_class: None,
        failure_code: None,
        ladder_step: None,
        attempt: None,
        uptime_ms: None,
    }
}

#[test]
fn transitional_states_force_recover_before_start() {
    for state in [
        "Recovering",
        "Starting",
        "Scanning",
        "Associating",
        "DhcpWait",
        "ListenerWait",
        "Failed",
    ] {
        assert!(
            should_force_recover_before_start(&net_status(Some(state))),
            "state={state} should force recover"
        );
    }
}

#[test]
fn stable_or_unknown_states_do_not_force_recover_before_start() {
    for state in [Some("Ready"), Some("Idle"), None] {
        assert!(
            !should_force_recover_before_start(&net_status(state)),
            "state={state:?} should not force recover"
        );
    }
}

#[test]
fn format_health_status_error_is_compact() {
    assert_eq!(
        format_health_status_error(StatusCode::SERVICE_UNAVAILABLE),
        "HTTP 503"
    );
}

#[test]
fn ready_without_listener_is_detected() {
    let mut status = net_status(Some("Ready"));
    status.link = Some(true);
    status.ipv4 = Some("192.168.1.5".to_string());
    status.listener_enabled = Some(true);
    status.listener = Some(false);
    assert!(is_ready_without_listener(&status));

    status.listener = Some(true);
    assert!(!is_ready_without_listener(&status));
}

#[test]
fn wait_ready_retry_classifier_matches_listener_and_dhcp_stalls() {
    assert!(should_retry_wait_ready_after_recover(
        "network failure class=listener_not_ready code=1 state=Some(\"Failed\")"
    ));
    assert!(should_retry_wait_ready_after_recover(
        "net_wait_ready: listener timeout"
    ));
    assert!(should_retry_wait_ready_after_recover(
        "dhcp/no-ipv4 stall: connected-without-ipv4 observed"
    ));
}

#[test]
fn wait_ready_retry_classifier_ignores_non_retryable_failures() {
    assert!(!should_retry_wait_ready_after_recover(
        "panic_detected class=guru line_index=42"
    ));
    assert!(!should_retry_wait_ready_after_recover(
        "net_wait_ready: overall timeout"
    ));
}

#[test]
fn runtime_health_metric_parser_reads_exact_keys() {
    let touch = "METRICS TOUCH_SCHED active_gap_max_ms=16";
    assert_eq!(metric_u32(touch, "active_gap_max_ms"), Some(16));
    assert_eq!(metric_u32(touch, "gap_max_ms"), None);

    let memory = "PSRAM min_internal_free_bytes=16968";
    assert_eq!(metric_u32(memory, "min_internal_free_bytes"), Some(16_968));

    let probe = "PSRAM internal_probe_stable=true";
    assert_eq!(metric_bool(probe, "internal_probe_stable"), Some(true));
    assert_eq!(metric_bool(probe, "missing"), None);
}

#[test]
fn runtime_health_requires_an_allocation_free_serving_snapshot() {
    let canonical = "PSRAM internal_free_bytes=17000 min_internal_free_bytes=16384 min_internal_alloc_charge_bytes=1700 min_internal_alloc_internal_required=true min_internal_alloc_charge_overflow=false min_internal_alloc_post_free_bytes=16384 min_internal_alloc_correlation_stable=true min_internal_alloc_wifi_rx_matched=true min_internal_alloc_released=true internal_probe_performed=false internal_probe_block_bytes=0 internal_probe_reserve_bytes=16384 internal_probe_free_before_bytes=17000 internal_probe_free_after_bytes=17000 internal_probe_stable=true";
    parse_serving_allocator_status(canonical).expect("canonical allocation-free snapshot");

    for invalid in [
        canonical.replace(
            "min_internal_alloc_charge_bytes=1700",
            "min_internal_alloc_charge_bytes=0",
        ),
        canonical.replace(
            "min_internal_alloc_internal_required=true",
            "min_internal_alloc_internal_required=invalid",
        ),
        canonical.replace(
            "min_internal_alloc_charge_overflow=false",
            "min_internal_alloc_charge_overflow=true",
        ),
        canonical.replace(
            "min_internal_alloc_correlation_stable=true",
            "min_internal_alloc_correlation_stable=false",
        ),
        canonical.replace(
            "min_internal_alloc_released=true",
            "min_internal_alloc_released=false",
        ),
        canonical.replace(
            "min_internal_alloc_post_free_bytes=16384",
            "min_internal_alloc_post_free_bytes=16392",
        ),
        canonical.replace("performed=false", "performed=true"),
        canonical.replace("block_bytes=0", "block_bytes=4112"),
        canonical.replace("reserve_bytes=16384", "reserve_bytes=8192"),
        canonical.replace("after_bytes=17000", "after_bytes=16992"),
        canonical.replace("before_bytes=17000", "before_bytes=16992"),
        canonical.replace(
            "min_internal_free_bytes=16384",
            "min_internal_free_bytes=18000",
        ),
        canonical.replace("stable=true", "stable=false"),
    ] {
        assert!(parse_serving_allocator_status(&invalid).is_err());
    }
}
