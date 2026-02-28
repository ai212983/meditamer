mod clock;
mod visual;

use embassy_time::Instant;

use super::{
    app_state::{BaseMode, DayBackground, OverlayMode},
    types::{InkplateDriver, TimeSyncState},
};

pub(crate) use clock::{render_clock_overlay, sample_battery_percent};
pub(crate) use visual::{next_visual_seed, render_shanshui_update, render_suminagashi_update};

pub(crate) type RenderTiming = (u32, Option<TimeSyncState>, Option<u8>);

pub(crate) async fn render_active_mode(
    display: &mut InkplateDriver,
    base_mode: BaseMode,
    day_background: DayBackground,
    overlay_mode: OverlayMode,
    timing: RenderTiming,
    seed_state: (&mut u32, &mut bool),
) {
    let (uptime_seconds, time_sync, battery_percent) = timing;
    let (pattern_nonce, first_visual_seed_pending) = seed_state;
    match base_mode {
        BaseMode::TouchWizard => {}
        BaseMode::Day => {
            let seed = next_visual_seed(
                uptime_seconds,
                time_sync,
                pattern_nonce,
                first_visual_seed_pending,
            );
            match day_background {
                DayBackground::Suminagashi => {
                    render_suminagashi_update(display, seed, uptime_seconds, time_sync).await;
                }
                DayBackground::Shanshui => {
                    render_shanshui_update(display, seed, uptime_seconds, time_sync).await;
                }
            }
            if matches!(overlay_mode, OverlayMode::Clock) {
                render_clock_overlay(display, uptime_seconds, time_sync, battery_percent).await;
            }
        }
    }
}

pub(crate) async fn render_visual_update(
    display: &mut InkplateDriver,
    day_background: DayBackground,
    overlay_mode: OverlayMode,
    timing: RenderTiming,
    seed_state: (&mut u32, &mut bool),
) {
    let (uptime_seconds, time_sync, battery_percent) = timing;
    let (pattern_nonce, first_visual_seed_pending) = seed_state;
    let seed = next_visual_seed(
        uptime_seconds,
        time_sync,
        pattern_nonce,
        first_visual_seed_pending,
    );
    match day_background {
        DayBackground::Suminagashi => {
            render_suminagashi_update(display, seed, uptime_seconds, time_sync).await
        }
        DayBackground::Shanshui => {
            render_shanshui_update(display, seed, uptime_seconds, time_sync).await
        }
    }
    if matches!(overlay_mode, OverlayMode::Clock) {
        render_clock_overlay(display, uptime_seconds, time_sync, battery_percent).await;
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
