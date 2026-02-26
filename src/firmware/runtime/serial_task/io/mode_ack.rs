use embassy_time::{with_timeout, Duration, Instant};

use crate::firmware::{
    config::RUNTIME_SERVICES_APPLY_ACKS,
    types::{RuntimeServices, RuntimeServicesApplyAck},
};

pub(crate) fn drain_runtime_services_apply_acks() {
    while RUNTIME_SERVICES_APPLY_ACKS.try_receive().is_ok() {}
}

pub(crate) async fn wait_runtime_services_apply_ack(
    request_id: u16,
    timeout_ms: u64,
) -> Option<RuntimeServices> {
    let start = Instant::now();
    loop {
        let elapsed_ms = Instant::now().saturating_duration_since(start).as_millis();
        if elapsed_ms >= timeout_ms {
            return None;
        }

        let remaining_ms = timeout_ms.saturating_sub(elapsed_ms).max(1);
        match with_timeout(
            Duration::from_millis(remaining_ms),
            RUNTIME_SERVICES_APPLY_ACKS.receive(),
        )
        .await
        {
            Ok(RuntimeServicesApplyAck {
                request_id: received_request_id,
                applied,
            }) if received_request_id == request_id => return Some(applied),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}
