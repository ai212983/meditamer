use embassy_time::{with_timeout, Duration, Instant};

use crate::firmware::{config::APP_STATE_APPLY_ACKS, types::AppStateApplyAck};

pub(crate) fn drain_app_state_apply_acks() {
    while APP_STATE_APPLY_ACKS.try_receive().is_ok() {}
}

pub(crate) async fn wait_app_state_apply_ack(
    request_id: u16,
    timeout_ms: u64,
) -> Option<AppStateApplyAck> {
    let start = Instant::now();
    loop {
        let elapsed_ms = Instant::now().saturating_duration_since(start).as_millis();
        if elapsed_ms >= timeout_ms {
            return None;
        }

        let remaining_ms = timeout_ms.saturating_sub(elapsed_ms).max(1);
        match with_timeout(
            Duration::from_millis(remaining_ms),
            APP_STATE_APPLY_ACKS.receive(),
        )
        .await
        {
            Ok(ack) if ack.request_id == request_id => return Some(ack),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}
