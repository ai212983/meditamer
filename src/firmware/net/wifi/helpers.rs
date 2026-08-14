use super::*;

pub(super) fn format_bssid(bssid: [u8; 6]) -> heapless::String<17> {
    let mut out = heapless::String::<17>::new();
    let _ = write!(
        out,
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        bssid[0], bssid[1], bssid[2], bssid[3], bssid[4], bssid[5]
    );
    out
}

pub(super) fn format_bssid_opt(bssid: Option<[u8; 6]>) -> heapless::String<17> {
    match bssid {
        Some(value) => format_bssid(value),
        None => {
            let mut out = heapless::String::<17>::new();
            let _ = out.push_str("<none>");
            out
        }
    }
}

pub(super) fn policy_total_attempt_budget(policy: WifiRuntimePolicy) -> u32 {
    u32::from(policy.retry_same_max)
        + u32::from(policy.rotate_candidate_max)
        + u32::from(policy.rotate_auth_max)
        + u32::from(policy.full_scan_reset_max)
        + u32::from(policy.driver_restart_max)
        + 1
}

pub(super) fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

pub(super) fn is_no_mem_wifi_error(err: &WifiError) -> bool {
    wifi_error_is_no_mem(err)
}

pub(super) fn log_radio_mem_diag(stage: &str) {
    log_radio_mem_diag_with_trigger(stage, "none");
}

pub(super) fn log_radio_mem_diag_with_trigger(stage: &str, trigger: &str) {
    let snapshot = psram::allocator_memory_snapshot();
    diag_reassoc!(
        "upload_http: radio_mem stage={} trigger={} feature={} state={:?} total={} used={} free={} peak={} internal_free={} external_free={} min_free={} min_internal_free={} min_external_free={} large_alloc_external_ok={} large_alloc_internal_ok={} large_alloc_fail={}",
        stage,
        trigger,
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

pub(super) fn stack_ipv4_lease(stack: &Stack<'_>) -> Option<[u8; 4]> {
    stack
        .config_v4()
        .map(|cfg| cfg.address.address().octets())
        .filter(|ip| *ip != [0, 0, 0, 0])
}

pub(super) fn has_ipv4_lease(stack: &Stack<'_>) -> bool {
    stack_ipv4_lease(stack).is_some()
}
