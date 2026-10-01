use embassy_time::Timer;

use crate::firmware::{
    config::{APP_EVENTS, SD_REQUESTS},
    types::{AppEvent, SdRequest},
};

const APP_EVENT_ENQUEUE_RETRY_MS: u64 = 25;
// Absorb short SD/FAT bursts without requiring host-side pacing.
const APP_EVENT_ENQUEUE_MAX_RETRIES: u8 = 240;

pub(super) async fn enqueue_app_event_with_retry(event: AppEvent) -> bool {
    for attempt in 0..=APP_EVENT_ENQUEUE_MAX_RETRIES {
        if APP_EVENTS.try_send(event).is_ok() {
            return true;
        }
        if attempt == APP_EVENT_ENQUEUE_MAX_RETRIES {
            break;
        }
        Timer::after_millis(APP_EVENT_ENQUEUE_RETRY_MS).await;
    }
    false
}

pub(super) async fn enqueue_sd_request_with_retry(request: SdRequest) -> bool {
    for attempt in 0..=APP_EVENT_ENQUEUE_MAX_RETRIES {
        if SD_REQUESTS.try_send(request).is_ok() {
            return true;
        }
        if attempt == APP_EVENT_ENQUEUE_MAX_RETRIES {
            break;
        }
        Timer::after_millis(APP_EVENT_ENQUEUE_RETRY_MS).await;
    }
    false
}
