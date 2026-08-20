use core::sync::atomic::Ordering;

use super::super::counters::*;
use super::super::types::{NetPipelineGate, UploadHttpPhaseMetrics};
use super::helpers::{saturating_add_u32, update_max_u32};

pub(crate) fn set_upload_http_listener(listening: bool, ip: Option<[u8; 4]>) {
    let previous = UPLOAD_HTTP_LISTENING.swap(listening, Ordering::Relaxed);
    arbitration::claim::set_service_listening(listening);
    if listening && !previous {
        NET_PIPELINE_LISTENER_ON.fetch_add(1, Ordering::Relaxed);
    } else if !listening && previous {
        NET_PIPELINE_LISTENER_OFF.fetch_add(1, Ordering::Relaxed);
    }
    let raw_ip = ip.map(u32::from_be_bytes).unwrap_or(0);
    UPLOAD_HTTP_IPV4.store(raw_ip, Ordering::Relaxed);
}

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

pub(crate) fn record_net_pipeline_dhcp_wait(elapsed_ms: u32) {
    NET_PIPELINE_DHCP_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&NET_PIPELINE_DHCP_WAIT_MS_TOTAL, elapsed_ms);
    update_max_u32(&NET_PIPELINE_DHCP_WAIT_MS_MAX, elapsed_ms);
}

pub(crate) fn record_net_pipeline_dhcp_ready() {
    NET_PIPELINE_DHCP_READY_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_net_pipeline_gate(reason: NetPipelineGate) {
    match reason {
        NetPipelineGate::WifiDown => {
            NET_PIPELINE_GATE_WIFI_DOWN.fetch_add(1, Ordering::Relaxed);
        }
        NetPipelineGate::LinkDown => {
            NET_PIPELINE_GATE_LINK_DOWN.fetch_add(1, Ordering::Relaxed);
        }
        NetPipelineGate::NoIpv4 => {
            NET_PIPELINE_GATE_NO_IPV4.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) fn record_net_pipeline_accept_wait(elapsed_ms: u32) {
    NET_PIPELINE_ACCEPT_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&NET_PIPELINE_ACCEPT_WAIT_MS_TOTAL, elapsed_ms);
    update_max_u32(&NET_PIPELINE_ACCEPT_WAIT_MS_MAX, elapsed_ms);
}

pub(crate) fn record_net_pipeline_accept_arm_gap(elapsed_us: u32, after_mkdir: bool) {
    NET_PIPELINE_ACCEPT_ARM_GAP_COUNT.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&NET_PIPELINE_ACCEPT_ARM_GAP_US_TOTAL, elapsed_us);
    update_max_u32(&NET_PIPELINE_ACCEPT_ARM_GAP_US_MAX, elapsed_us);
    if after_mkdir {
        NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_COUNT.fetch_add(1, Ordering::Relaxed);
        saturating_add_u32(
            &NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_US_TOTAL,
            elapsed_us,
        );
        update_max_u32(&NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_US_MAX, elapsed_us);
    }
}
