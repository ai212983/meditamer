use embassy_time::Instant;

use crate::app::{
    render::render_active_mode,
    runtime::trigger_backlight_cycle,
    touch::config::TOUCH_FEEDBACK_ENABLED,
    touch::tasks::draw_touch_feedback_dot,
    touch::types::{TouchEvent, TouchEventKind, TouchSwipeDirection},
    types::{DisplayContext, DisplayMode, TimeSyncState},
};

pub(crate) async fn handle_touch_event(
    event: TouchEvent,
    context: &mut DisplayContext,
    touch_feedback_dirty: &mut bool,
    backlight_cycle_start: &mut Option<Instant>,
    backlight_level: &mut u8,
    update_count: &mut u32,
    display_mode: &mut DisplayMode,
    last_uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    battery_percent: Option<u8>,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
    screen_initialized: &mut bool,
) {
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
                pattern_nonce,
                first_visual_seed_pending,
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
                pattern_nonce,
                first_visual_seed_pending,
                true,
            )
            .await;
            *screen_initialized = true;
        }
        _ => {}
    }
}
