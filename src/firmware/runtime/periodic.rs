use embassy_time::{Duration, Instant, Ticker};

use super::super::{
    config::{APP_EVENTS, BATTERY_INTERVAL_SECONDS, REFRESH_INTERVAL_SECONDS},
    types::AppEvent,
};

#[embassy_executor::task]
pub(crate) async fn clock_task() {
    let boot_instant = Instant::now();
    APP_EVENTS
        .send(AppEvent::Refresh { uptime_seconds: 0 })
        .await;
    let mut ticker = Ticker::every(Duration::from_secs(REFRESH_INTERVAL_SECONDS as u64));

    loop {
        ticker.next().await;
        let uptime_seconds = Instant::now()
            .saturating_duration_since(boot_instant)
            .as_secs()
            .min(u32::MAX as u64) as u32;
        APP_EVENTS.send(AppEvent::Refresh { uptime_seconds }).await;
    }
}

#[embassy_executor::task]
pub(crate) async fn battery_task() {
    APP_EVENTS.send(AppEvent::BatteryTick).await;
    let mut ticker = Ticker::every(Duration::from_secs(BATTERY_INTERVAL_SECONDS as u64));

    loop {
        ticker.next().await;
        APP_EVENTS.send(AppEvent::BatteryTick).await;
    }
}
