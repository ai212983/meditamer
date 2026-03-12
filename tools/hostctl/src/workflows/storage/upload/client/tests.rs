use std::{io::ErrorKind, time::Duration};

use anyhow::{anyhow, Context};
use reqwest::blocking::Client;

use super::{
    errors::{format_error_chain, inspect_io_error_flags, inspect_reqwest_error_flags, is_transport_reset_error},
    request::health_timeout_s,
    retry::is_transport_reset_chunk_fallback_error,
    TRANSPORT_RESET_CHUNK_FALLBACK_MARKER,
};

#[test]
fn health_timeout_is_clamped() {
    assert_eq!(health_timeout_s(0.01), 0.5);
    assert_eq!(health_timeout_s(1.25), 1.25);
    assert_eq!(health_timeout_s(999.0), 5.0);
}

#[test]
fn transport_reset_error_detection_matches_send_and_reset_signatures() {
    assert!(is_transport_reset_error(
        "put http://10.0.0.8:8080/upload send failed"
    ));
    assert!(is_transport_reset_error("connection reset by peer"));
    assert!(!is_transport_reset_error("409 sd busy"));
}

#[test]
fn reqwest_and_io_flag_extractors_capture_nested_error_types() {
    let io_err = std::io::Error::new(ErrorKind::ConnectionReset, "reset");
    let err = anyhow::Error::from(io_err).context("PUT http://10.0.0.8:8080/upload send failed");
    let io_flags = inspect_io_error_flags(&err);
    assert!(io_flags.connection_reset);
    assert!(io_flags.transient());

    let client = Client::builder()
        .timeout(Duration::from_millis(100))
        .build()
        .expect("client");
    let reqwest_err = client
        .get("http://127.0.0.1:1/health")
        .send()
        .expect_err("request must fail");
    let wrapped = anyhow::Error::from(reqwest_err).context("GET /health send failed");
    let req_flags = inspect_reqwest_error_flags(&wrapped);
    assert!(req_flags.seen);
    assert!(req_flags.transient());
}

#[test]
fn error_chain_formatter_keeps_context_order() {
    let err: anyhow::Error = Err::<(), _>(anyhow!("leaf network error"))
        .context("mid request layer")
        .context("top upload wrapper")
        .expect_err("must fail");
    let chain = format_error_chain(&err, 8);
    assert!(chain.contains("top upload wrapper"));
    assert!(chain.contains("mid request layer"));
    assert!(chain.contains("leaf network error"));
}

#[test]
fn transport_reset_chunk_fallback_marker_detection_is_explicit() {
    let marked = anyhow!("send failed").context(format!(
        "{}: streak=3 limit=2 attempt=4",
        TRANSPORT_RESET_CHUNK_FALLBACK_MARKER
    ));
    assert!(is_transport_reset_chunk_fallback_error(&marked));
    let plain = anyhow!("send failed without fallback marker");
    assert!(!is_transport_reset_chunk_fallback_error(&plain));
}
