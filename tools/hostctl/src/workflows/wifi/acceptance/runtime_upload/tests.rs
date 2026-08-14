use super::helpers::{
    append_health_fail_net_status, append_panic_signal_context, classify_host_upload_failure,
    parse_metrics_key_u32, refresh_retry_eligible_host_failure,
};
use crate::workflows::wifi::common::detect_panic_signal;

#[test]
fn health_failure_detail_includes_net_status_snapshot() {
    let mut detail = "health check failed: GET http://10.0.0.8:8080/health".to_string();
    let diag = append_health_fail_net_status(
        &mut detail,
        Ok(Some(
            "NET_STATUS {\"state\":\"Ready\",\"link\":true,\"ipv4\":\"10.0.0.8\"}".to_string(),
        )),
    );
    assert!(diag.contains("health_fail_diag: NET_STATUS"));
    assert!(detail.contains("net_status=NET_STATUS"));
}

#[test]
fn health_failure_detail_records_query_error_and_panic_context() {
    let mut detail = "health check failed: GET http://10.0.0.8:8080/health".to_string();
    let diag = append_health_fail_net_status(&mut detail, Err("serial read timed out".to_string()));
    assert!(diag.contains("NET_STATUS query failed"));
    assert!(detail.contains("net_status_query_error=serial read timed out"));

    let signal = detect_panic_signal("Guru Meditation Error: Core 0 panic'ed", 42)
        .expect("panic signal must be detected");
    assert!(append_panic_signal_context(&mut detail, Some(&signal)));
    assert!(detail.contains("panic_class=runtime_panic_guru"));
    assert!(detail.contains("panic_line_index=42"));
}

#[test]
fn parse_req_read_body_reset_metric_from_upload_line() {
    let line = "METRICS UPLOAD accept_ok=10 accept_err=0 request_err=1 req_hdr_to=0 req_read_body=1 req_read_body_reset=1 req_sd_busy=0 sd_errors=0";
    assert_eq!(parse_metrics_key_u32(line, "req_read_body_reset"), Some(1));
    assert_eq!(parse_metrics_key_u32(line, "req_read_body"), Some(1));
    assert_eq!(parse_metrics_key_u32(line, "missing"), None);
}

#[test]
fn classify_host_upload_failure_signatures() {
    assert_eq!(
        classify_host_upload_failure(
            "health check failed: GET http://10.0.0.8:8080/health last_error=GET ... send failed"
        ),
        Some("host_health_send_fail")
    );
    assert_eq!(
        classify_host_upload_failure(
            "PUT http://10.0.0.8:8080/upload send failed; connection refused (os error 61)"
        ),
        Some("host_transport_connect_refused")
    );
    assert_eq!(
        classify_host_upload_failure("PUT http://10.0.0.8:8080/upload send failed"),
        Some("host_transport_send_fail")
    );
    assert_eq!(
        classify_host_upload_failure("connection reset by peer"),
        Some("host_transport_connection_reset")
    );
    assert_eq!(classify_host_upload_failure("remote verify failed"), None);
}

#[test]
fn refresh_retry_eligibility_covers_send_and_reset_classes() {
    assert!(refresh_retry_eligible_host_failure("host_health_send_fail"));
    assert!(refresh_retry_eligible_host_failure(
        "host_transport_send_fail"
    ));
    assert!(refresh_retry_eligible_host_failure(
        "host_transport_connection_reset"
    ));
    assert!(!refresh_retry_eligible_host_failure(
        "host_transport_connect_refused"
    ));
}
