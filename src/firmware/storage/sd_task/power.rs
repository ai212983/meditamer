use embassy_time::{with_timeout, Duration, Instant, Timer};

use super::super::super::config::{SD_POWER_REQUESTS, SD_POWER_RESPONSES};
use super::{
    sd_power_action_label, SdPowerRequest, SD_BACKOFF_BASE_MS, SD_BACKOFF_MAX_MS,
    SD_POWER_OFF_RESPONSE_TIMEOUT_MS, SD_POWER_ON_RESPONSE_TIMEOUT_MS,
    SD_POWER_REQUEST_ENQUEUE_TIMEOUT_MS, SD_POWER_REQUEST_MAX_ATTEMPTS,
    SD_POWER_REQUEST_RETRY_DELAY_MS,
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
    let action_label = sd_power_action_label(action);
    let response_timeout_ms = match action {
        SdPowerRequest::On => SD_POWER_ON_RESPONSE_TIMEOUT_MS,
        SdPowerRequest::Off => SD_POWER_OFF_RESPONSE_TIMEOUT_MS,
    };
    let mut attempt = 1u8;
    while attempt <= SD_POWER_REQUEST_MAX_ATTEMPTS {
        while SD_POWER_RESPONSES.try_receive().is_ok() {}

        if with_timeout(
            Duration::from_millis(SD_POWER_REQUEST_ENQUEUE_TIMEOUT_MS),
            SD_POWER_REQUESTS.send(action),
        )
        .await
        .is_err()
        {
            esp_println::println!(
                "sdtask: power_req_enqueue_timeout action={} timeout_ms={} attempt={}/{}",
                action_label,
                SD_POWER_REQUEST_ENQUEUE_TIMEOUT_MS,
                attempt,
                SD_POWER_REQUEST_MAX_ATTEMPTS,
            );
        } else {
            match with_timeout(
                Duration::from_millis(response_timeout_ms),
                SD_POWER_RESPONSES.receive(),
            )
            .await
            {
                Ok(ok) => return ok,
                Err(_) => {
                    esp_println::println!(
                        "sdtask: power_resp_timeout action={} timeout_ms={} attempt={}/{}",
                        action_label,
                        response_timeout_ms,
                        attempt,
                        SD_POWER_REQUEST_MAX_ATTEMPTS,
                    );
                }
            }
        }

        if attempt < SD_POWER_REQUEST_MAX_ATTEMPTS {
            Timer::after_millis(SD_POWER_REQUEST_RETRY_DELAY_MS).await;
        }
        attempt = attempt.saturating_add(1);
    }

    esp_println::println!(
        "sdtask: power_request_failed action={} attempts={}",
        action_label,
        SD_POWER_REQUEST_MAX_ATTEMPTS
    );
    false
}
