//! HTTP request/body/SD-roundtrip timing recorders.
//!
//! Split out of this file's former mixed upload/network recorder in the
//! product/target axis completion plan's Phase 4: listener bind state and
//! network-pipeline counters (DHCP wait, gate reasons, accept timing) moved
//! to `platform/connectivity/netstack::telemetry` -- they are network-owned. This half
//! stayed here: it is the product's own upload service's request/body/SD
//! timing, not network operational state.

use core::sync::atomic::Ordering;

use super::super::counters::*;
use super::super::types::UploadHttpPhaseMetrics;
use super::helpers::{saturating_add_u32, update_max_u32};

pub(crate) fn record_upload_http_accept() {
    UPLOAD_HTTP_ACCEPTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::trace!("telemetry upload_http_accept");
}

pub(crate) fn record_upload_http_accept_error() {
    UPLOAD_HTTP_ACCEPT_ERRORS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry upload_http_accept_error");
}

pub(crate) fn record_upload_http_accept_link_reset() {
    UPLOAD_HTTP_ACCEPT_LINK_RESETS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry upload_http_accept_link_reset");
}

pub(crate) fn record_upload_http_request_error() {
    UPLOAD_HTTP_REQUEST_ERRORS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry upload_http_request_error");
}

pub(crate) fn record_upload_http_read_body_reset() {
    UPLOAD_HTTP_READ_BODY_RESETS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry upload_http_read_body_reset");
}

pub(crate) fn record_upload_http_request_bucket(error: &'static str) {
    match error {
        "request header timeout" => {
            UPLOAD_HTTP_HEADER_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
        }
        "read body" => {
            UPLOAD_HTTP_READ_BODY_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
        "sd busy" => {
            UPLOAD_HTTP_SD_BUSY_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

pub(crate) fn record_upload_http_health_request() {
    UPLOAD_HTTP_HEALTH_REQUESTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::trace!("telemetry upload_http_health_request");
}

pub(crate) fn record_upload_http_upload_phase(metrics: UploadHttpPhaseMetrics) {
    UPLOAD_HTTP_UPLOAD_REQUESTS.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_BYTES, metrics.bytes);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_BODY_READ_MS_TOTAL, metrics.body_read_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_BODY_READ_MS_MAX, metrics.body_read_ms);
    saturating_add_u32(
        &UPLOAD_HTTP_UPLOAD_PAYLOAD_COPY_MS_TOTAL,
        metrics.payload_copy_ms,
    );
    update_max_u32(
        &UPLOAD_HTTP_UPLOAD_PAYLOAD_COPY_MS_MAX,
        metrics.payload_copy_ms,
    );
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_SD_QUEUE_MS_TOTAL, metrics.sd_queue_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_SD_QUEUE_MS_MAX, metrics.sd_queue_ms);
    saturating_add_u32(
        &UPLOAD_HTTP_UPLOAD_SD_TASK_WAIT_MS_TOTAL,
        metrics.sd_task_wait_ms,
    );
    update_max_u32(
        &UPLOAD_HTTP_UPLOAD_SD_TASK_WAIT_MS_MAX,
        metrics.sd_task_wait_ms,
    );
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_COMMIT_MS_TOTAL, metrics.commit_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_COMMIT_MS_MAX, metrics.commit_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_CHUNK_P50_MS_MAX, metrics.chunk_p50_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_CHUNK_P95_MS_MAX, metrics.chunk_p95_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_CHUNK_MAX_MS_MAX, metrics.chunk_max_ms);
    saturating_add_u32(
        &UPLOAD_HTTP_UPLOAD_CHUNK_SAMPLES_TOTAL,
        metrics.chunk_samples,
    );
    saturating_add_u32(
        &UPLOAD_HTTP_UPLOAD_CHUNK_SAMPLES_DROPPED,
        metrics.chunk_samples_dropped,
    );
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_TOTAL, metrics.sd_wait_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_MAX, metrics.sd_wait_ms);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_REQUEST_MS_TOTAL, metrics.request_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_REQUEST_MS_MAX, metrics.request_ms);
}
