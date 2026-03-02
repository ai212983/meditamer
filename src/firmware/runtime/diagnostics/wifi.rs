async fn run_wifi_check(kind: DiagKind, targets: u8) -> Option<SessionOutcome> {
    if let Some(outcome) = session_interrupt_outcome(kind, targets) {
        return Some(outcome);
    }

    set_status(STATE_RUNNING, STEP_WIFI_READY, CODE_OK, targets);
    let snapshot = crate::firmware::app_state::read_app_state_snapshot();
    if !snapshot.services.upload_enabled {
        return Some(SessionOutcome::Failed(CODE_WIFI_DISABLED));
    }

    match wait_for_wifi_ready(kind, targets).await {
        Ok(true) => None,
        Ok(false) => Some(SessionOutcome::Failed(CODE_WIFI_NOT_READY)),
        Err(outcome) => Some(outcome),
    }
}

async fn wait_for_wifi_ready(kind: DiagKind, targets: u8) -> Result<bool, SessionOutcome> {
    let mut elapsed_ms = 0u64;
    while elapsed_ms < DIAG_WIFI_TIMEOUT_MS {
        if let Some(outcome) = session_interrupt_outcome(kind, targets) {
            return Err(outcome);
        }
        if telemetry::snapshot().wifi_link_connected {
            return Ok(true);
        }
        let wait_ms = (DIAG_WIFI_TIMEOUT_MS - elapsed_ms).min(DIAG_POLL_MS);
        Timer::after(Duration::from_millis(wait_ms)).await;
        elapsed_ms = elapsed_ms.saturating_add(wait_ms);
    }
    Ok(telemetry::snapshot().wifi_link_connected)
}
