use super::super::types::{InkplateDriver, TimeSyncState};

pub(crate) async fn render_clock_overlay(
    _display: &mut InkplateDriver,
    _uptime_seconds: u32,
    _time_sync: Option<TimeSyncState>,
    _battery_percent: Option<u8>,
) {
}

pub(crate) async fn sample_battery_percent(display: &mut InkplateDriver) -> Option<u8> {
    let soc = display.fuel_gauge_soc().await.ok()?;
    if soc > 100 {
        return None;
    }
    Some(soc as u8)
}
