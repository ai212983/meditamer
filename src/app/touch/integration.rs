use embassy_time::Instant;

use crate::app::{
    render::render_active_mode,
    runtime::trigger_backlight_cycle,
    touch::config::TOUCH_FEEDBACK_ENABLED,
    touch::tasks::draw_touch_feedback_dot,
    touch::types::{TouchEvent, TouchEventKind, TouchSwipeDirection},
    types::{DisplayContext, DisplayMode, TimeSyncState},
};

pub(crate) struct TouchEventContext<'a> {
    pub(crate) touch_feedback_dirty: &'a mut bool,
    pub(crate) backlight_cycle_start: &'a mut Option<Instant>,
    pub(crate) backlight_level: &'a mut u8,
    pub(crate) update_count: &'a mut u32,
    pub(crate) display_mode: &'a mut DisplayMode,
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
) {
    let TouchEventContext {
        touch_feedback_dirty,
        backlight_cycle_start,
        backlight_level,
        update_count,
        display_mode,
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
            render_active_mode(
                &mut context.inkplate,
                *display_mode,
                last_uptime_seconds,
                time_sync,
                battery_percent,
                seed_state,
                true,
            )
            .await;
            *screen_initialized = true;
        }
        TouchEventKind::Swipe(direction) => {
            *display_mode = match direction {
                TouchSwipeDirection::Right | TouchSwipeDirection::Down => display_mode.toggled(),
                TouchSwipeDirection::Left | TouchSwipeDirection::Up => {
                    display_mode.toggled_reverse()
                }
            };
            context.mode_store.save_mode(*display_mode);
            *update_count = 0;
            render_active_mode(
                &mut context.inkplate,
                *display_mode,
                last_uptime_seconds,
                time_sync,
                battery_percent,
                seed_state,
                true,
            )
            .await;
            *screen_initialized = true;
        }
        _ => {}
    }
}
