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

pub(crate) fn record_upload_http_upload_phase(
    bytes: u32,
    body_read_ms: u32,
    sd_wait_ms: u32,
    request_ms: u32,
) {
    UPLOAD_HTTP_UPLOAD_REQUESTS.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_BYTES, bytes);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_BODY_READ_MS_TOTAL, body_read_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_BODY_READ_MS_MAX, body_read_ms);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_TOTAL, sd_wait_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_MAX, sd_wait_ms);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_REQUEST_MS_TOTAL, request_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_REQUEST_MS_MAX, request_ms);
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
