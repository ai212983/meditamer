//! Battery-tick timing: the periodic `AppEvent::BatteryTick` producer.

use embassy_time::{Duration, Ticker};

use super::{
    config::{APP_EVENTS, BATTERY_INTERVAL_SECONDS},
    types::AppEvent,
};

#[embassy_executor::task]
pub(crate) async fn battery_task() {
    APP_EVENTS.send(AppEvent::BatteryTick).await;
    let mut ticker = Ticker::every(Duration::from_secs(BATTERY_INTERVAL_SECONDS as u64));

    loop {
        ticker.next().await;
        APP_EVENTS.send(AppEvent::BatteryTick).await;
    }
}
