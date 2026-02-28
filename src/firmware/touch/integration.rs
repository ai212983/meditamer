use embassy_time::Instant;

use super::super::{
    render::{render_active_mode, RenderActiveParams},
    runtime::trigger_backlight_cycle,
    types::{DisplayContext, TimeSyncState},
};
use super::{
    super::app_state::{AppStateCommand, BaseMode, DayBackground, OverlayMode},
    config::TOUCH_FEEDBACK_ENABLED,
    tasks::draw_touch_feedback_dot,
    types::{TouchEvent, TouchEventKind},
};

pub(crate) struct TouchEventContext<'a> {
    pub(crate) touch_feedback_dirty: &'a mut bool,
    pub(crate) backlight_cycle_start: &'a mut Option<Instant>,
    pub(crate) backlight_level: &'a mut u8,
    pub(crate) update_count: &'a mut u32,
    pub(crate) base_mode: BaseMode,
    pub(crate) day_background: DayBackground,
    pub(crate) overlay_mode: OverlayMode,
    pub(crate) last_uptime_seconds: u32,
    pub(crate) time_sync: Option<TimeSyncState>,
    pub(crate) battery_percent: Option<u8>,
    pub(crate) seed_state: (&'a mut u32, &'a mut bool),
    pub(crate) screen_initialized: &'a mut bool,
}

pub(crate) async fn handle_touch_event(
    event: TouchEvent,
    context: &mut DisplayContext,
    event_context: TouchEventContext<'_>,
) -> Option<AppStateCommand> {
    let TouchEventContext {
        touch_feedback_dirty,
        backlight_cycle_start,
        backlight_level,
        update_count,
        base_mode,
        day_background,
        overlay_mode,
        last_uptime_seconds,
        time_sync,
        battery_percent,
        seed_state,
        screen_initialized,
    } = event_context;
    match event.kind {
        TouchEventKind::Down | TouchEventKind::Move if TOUCH_FEEDBACK_ENABLED => {
            draw_touch_feedback_dot(&mut context.inkplate, event.x, event.y);
            *touch_feedback_dirty = true;
        }
        TouchEventKind::Tap => {
            trigger_backlight_cycle(
                &mut context.inkplate,
                backlight_cycle_start,
                backlight_level,
            );
        }
        TouchEventKind::LongPress => {
            *update_count = 0;
            let (pattern_nonce, first_visual_seed_pending) = seed_state;
            render_active_mode(
                &mut context.inkplate,
                RenderActiveParams {
                    base_mode,
                    day_background,
                    overlay_mode,
                    uptime_seconds: last_uptime_seconds,
                    time_sync,
                    battery_percent,
                    pattern_nonce,
                    first_visual_seed_pending,
                },
            )
            .await;
            *screen_initialized = true;
        }
        TouchEventKind::Swipe(_) => {
            return Some(AppStateCommand::ToggleDayBackground);
        }
        _ => {}
    }
    None
}
