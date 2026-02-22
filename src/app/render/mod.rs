mod clock;
mod visual;

use embassy_time::Instant;

use super::types::{DisplayMode, InkplateDriver, TimeSyncState};

pub(crate) use clock::{render_battery_update, render_clock_update, sample_battery_percent};
pub(crate) use visual::{next_visual_seed, render_shanshui_update, render_suminagashi_update};

pub(crate) async fn render_active_mode(
    display: &mut InkplateDriver,
    mode: DisplayMode,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    battery_percent: Option<u8>,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
    force_full: bool,
) {
    match mode {
        DisplayMode::Clock => render_clock_update(
            display,
            uptime_seconds,
            time_sync,
            battery_percent,
            force_full,
        ),
        DisplayMode::Suminagashi => {
            let seed = next_visual_seed(
                uptime_seconds,
                time_sync,
                pattern_nonce,
                first_visual_seed_pending,
            );
            render_suminagashi_update(display, seed, uptime_seconds, time_sync).await;
        }
        DisplayMode::Shanshui => {
            let seed = next_visual_seed(
                uptime_seconds,
                time_sync,
                pattern_nonce,
                first_visual_seed_pending,
            );
            render_shanshui_update(display, seed, uptime_seconds, time_sync).await;
        }
    }
}

pub(crate) async fn render_visual_update(
    display: &mut InkplateDriver,
    mode: DisplayMode,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
) {
    let seed = next_visual_seed(
        uptime_seconds,
        time_sync,
        pattern_nonce,
        first_visual_seed_pending,
    );
    match mode {
        DisplayMode::Clock => {}
        DisplayMode::Suminagashi => {
            render_suminagashi_update(display, seed, uptime_seconds, time_sync).await
        }
        DisplayMode::Shanshui => {
            render_shanshui_update(display, seed, uptime_seconds, time_sync).await
        }
    }
}

pub(crate) fn local_seconds_since_epoch(
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) -> u64 {
    if let Some(sync) = time_sync {
        let elapsed = Instant::now()
            .saturating_duration_since(sync.sync_instant)
            .as_secs();
        let utc_now = sync.unix_epoch_utc_seconds.saturating_add(elapsed);
        (utc_now as i64 + (sync.tz_offset_minutes as i64) * 60).max(0) as u64
    } else {
        let monotonic = Instant::now().as_secs().min(u32::MAX as u64) as u32;
        monotonic.max(uptime_seconds) as u64
    }
}
