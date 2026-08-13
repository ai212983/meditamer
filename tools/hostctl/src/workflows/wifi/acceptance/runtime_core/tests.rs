use super::health::{
    format_health_status_error, is_ready_without_listener, should_force_recover_before_start,
    should_retry_wait_ready_after_recover,
};
use super::start::metric_u32;
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
}
