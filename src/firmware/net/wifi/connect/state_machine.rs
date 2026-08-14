use super::*;
pub(super) fn apply_pending_runtime_policy_updates(runtime_policy: &mut WifiRuntimePolicy) {
    while let Ok(updated) = WIFI_RUNTIME_POLICY_UPDATES.try_receive() {
        let sanitized = updated.sanitized();
        if sanitized == *runtime_policy {
            continue;
        }
        *runtime_policy = sanitized;
        diag_wifi!(
            "upload_http: runtime wifi policy updated connect_timeout_ms={} dhcp_timeout_ms={} pinned_dhcp_timeout_ms={} listener_timeout_ms={}",
            runtime_policy.connect_timeout_ms,
            runtime_policy.dhcp_timeout_ms,
            runtime_policy.pinned_dhcp_timeout_ms,
            runtime_policy.listener_timeout_ms
        );
    }
}

pub(super) fn transition_state(
    current: &mut NetState,
    next: NetState,
    trigger: &str,
    started_at: Instant,
    ladder_step: RecoveryLadderStep,
    net_attempt: u32,
    failure: (NetFailureClass, u8),
) {
    if *current == next {
        return;
    }
    task::emit_net_event(*current, next, trigger, started_at);
    if let Some(stage) = state_mem_stage(next) {
        log_radio_mem_diag_with_trigger(stage, trigger);
    }
    *current = next;
    publish_state(
        *current,
        ladder_step,
        net_attempt,
        failure.0,
        failure.1,
        started_at.elapsed().as_millis() as u32,
    );
}
