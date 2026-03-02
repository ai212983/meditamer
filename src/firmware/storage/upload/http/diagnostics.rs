fn dhcp_ipv4_status(stack: &Stack<'static>) -> Result<[u8; 4], telemetry::NetPipelineGate> {
    if !telemetry::wifi_link_connected() || !stack.is_link_up() {
        if !telemetry::wifi_link_connected() {
            return Err(telemetry::NetPipelineGate::WifiDown);
        }
        return Err(telemetry::NetPipelineGate::LinkDown);
    }
    stack
        .config_v4()
        .map(|cfg| cfg.address.address().octets())
        .filter(|ip| *ip != [0, 0, 0, 0])
        .ok_or(telemetry::NetPipelineGate::NoIpv4)
}

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

fn log_http_mem_diag(stage: &str) {
    if !telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP)
        && !telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET)
    {
        return;
    }
    let snapshot = psram::allocator_memory_snapshot();
    esp_println::println!(
        "upload_http: upload_mem stage={} feature={} state={:?} total={} used={} free={} peak={} internal_free={} external_free={} min_free={} min_internal_free={} min_external_free={} large_alloc_external_ok={} large_alloc_internal_ok={} large_alloc_fail={}",
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
        snapshot.large_alloc_fail
    );
}
