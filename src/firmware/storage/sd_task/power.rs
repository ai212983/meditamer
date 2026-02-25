use embassy_time::{with_timeout, Duration, Instant};

use super::super::super::config::{SD_POWER_REQUESTS, SD_POWER_RESPONSES};
use super::{
    sd_power_action_label, SdPowerRequest, SD_BACKOFF_BASE_MS, SD_BACKOFF_MAX_MS,
    SD_POWER_RESPONSE_TIMEOUT_MS,
};

pub(crate) fn duration_ms_since(start: Instant) -> u32 {
    Instant::now()
        .saturating_duration_since(start)
        .as_millis()
        .min(u32::MAX as u64) as u32
}

pub(crate) fn failure_backoff_ms(consecutive_failures: u8) -> u64 {
    let exponent = consecutive_failures.saturating_sub(1).min(6);
    let factor = 1u64 << exponent;
    SD_BACKOFF_BASE_MS
        .saturating_mul(factor)
        .min(SD_BACKOFF_MAX_MS)
}

pub(crate) async fn request_sd_power(action: SdPowerRequest) -> bool {
    while SD_POWER_RESPONSES.try_receive().is_ok() {}

    if SD_POWER_REQUESTS.try_send(action).is_err() {
        esp_println::println!(
            "sdtask: power_req_queue_full action={}",
            sd_power_action_label(action)
        );
        return false;
    }

    match with_timeout(
        Duration::from_millis(SD_POWER_RESPONSE_TIMEOUT_MS),
        SD_POWER_RESPONSES.receive(),
    )
    .await
    {
        Ok(ok) => ok,
        Err(_) => {
            esp_println::println!(
                "sdtask: power_resp_timeout action={} timeout_ms={}",
                sd_power_action_label(action),
                SD_POWER_RESPONSE_TIMEOUT_MS
            );
            false
        }
    }
}
