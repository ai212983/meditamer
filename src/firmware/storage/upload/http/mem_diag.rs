use crate::firmware::observability;
use crate::firmware::psram;

pub(super) fn log_http_mem_diag(stage: &str) {
    if !observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP)
        && !observability::log_filter_enabled(observability::LOG_DOMAIN_NET)
    {
        return;
    }
    let rx_before = crate::firmware::net::wifi::wifi_rx_buffer_stats();
    let snapshot = psram::allocator_memory_snapshot();
    let rx_after = crate::firmware::net::wifi::wifi_rx_buffer_stats();
    let rx_window_stable = rx_before.live == rx_after.live
        && rx_before.payload_bytes_live == rx_after.payload_bytes_live
        && rx_before.created == rx_after.created
        && rx_before.dropped == rx_after.dropped;
    esp_println::println!(
        "upload_http: upload_mem stage={} feature={} state={:?} total={} used={} free={} peak={} internal_free={} external_free={} min_free={} min_internal_free={} min_external_free={} large_alloc_external_ok={} large_alloc_internal_ok={} large_alloc_fail={} rx_window_stable={} rx_live={} rx_peak={} rx_payload_live={} rx_payload_peak={} rx_created={} rx_dropped={}",
        stage,
        snapshot.feature_enabled,
        snapshot.state,
        snapshot.total_bytes,
        snapshot.used_bytes,
        snapshot.free_bytes,
        snapshot.peak_used_bytes,
        snapshot.free_internal_bytes,
        snapshot.free_external_bytes,
        snapshot.min_free_bytes,
        snapshot.min_free_internal_bytes,
        snapshot.min_free_external_bytes,
        snapshot.large_alloc_external_ok,
        snapshot.large_alloc_internal_ok,
        snapshot.large_alloc_fail,
        rx_window_stable,
        rx_after.live,
        rx_after.peak,
        rx_after.payload_bytes_live,
        rx_after.payload_bytes_peak,
        rx_after.created,
        rx_after.dropped,
    );
}
