use embassy_time::{Duration, Instant};

use super::super::super::{
    app_state::{AppStateCommand, BaseMode},
    render::{render_active_mode, RenderActiveParams},
    touch::{
        config::{TOUCH_FEEDBACK_MIN_REFRESH_MS, TOUCH_PIPELINE_EVENTS},
        integration::{handle_touch_event, TouchEventContext},
        types::TouchEventKind,
        wizard::WizardDispatch,
    },
    types::DisplayContext,
};
use super::state::DisplayLoopState;

pub(super) async fn process_touch_cycle(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    while let Ok(touch_event) = TOUCH_PIPELINE_EVENTS.try_receive() {
        match touch_event.kind {
            TouchEventKind::Down | TouchEventKind::Move | TouchEventKind::LongPress => {
                state.touch_contact_active = true;
                state.touch_last_nonzero_at = Some(Instant::now());
            }
            TouchEventKind::Up
            | TouchEventKind::Tap
            | TouchEventKind::Swipe(_)
            | TouchEventKind::Cancel => {
                state.touch_contact_active = false;
                state.touch_last_nonzero_at = None;
            }
        }

        if state.touch_wizard.is_active() {
            match state
                .touch_wizard
                .handle_event(&mut context.inkplate, touch_event)
                .await
            {
                WizardDispatch::Inactive => {}
                WizardDispatch::Consumed => continue,
                WizardDispatch::Finished => {
                    let _ = state
                        .apply_state_command(context, AppStateCommand::SetBase(BaseMode::Day))
                        .await;
                    state.update_count = 0;
                    render_active_mode_from_state(context, state).await;
                    continue;
                }
            }
        }

        let last_uptime_seconds = state.last_uptime_seconds;
        let time_sync = state.time_sync;
        let battery_percent = state.battery_percent;
        let base_mode = state.base_mode();
        let day_background = state.day_background();
        let overlay_mode = state.overlay_mode();
        if let Some(command) = handle_touch_event(
            touch_event,
            context,
            TouchEventContext {
                touch_feedback_dirty: &mut state.touch_feedback_dirty,
                backlight_cycle_start: &mut state.backlight_cycle_start,
                backlight_level: &mut state.backlight_level,
                update_count: &mut state.update_count,
                base_mode,
                day_background,
                overlay_mode,
                last_uptime_seconds,
                time_sync,
                battery_percent,
                seed_state: (
                    &mut state.pattern_nonce,
                    &mut state.first_visual_seed_pending,
                ),
                screen_initialized: &mut state.screen_initialized,
            },
        )
        .await
        {
            let result = state.apply_state_command(context, command).await;
            if result.changed() && !state.in_touch_wizard_mode() {
                render_active_mode_from_state(context, state).await;
            }
        }
    }

    if state.touch_feedback_dirty
        && !state.touch_contact_active
        && Instant::now() >= state.touch_feedback_next_flush_at
    {
        let _ = context.inkplate.display_bw_partial_async(false).await;
        state.touch_feedback_dirty = false;
        state.touch_feedback_next_flush_at =
            Instant::now() + Duration::from_millis(TOUCH_FEEDBACK_MIN_REFRESH_MS);
    }
}

async fn render_active_mode_from_state(context: &mut DisplayContext, state: &mut DisplayLoopState) {
    render_active_mode(
        &mut context.inkplate,
        RenderActiveParams {
            base_mode: state.base_mode(),
            day_background: state.day_background(),
            overlay_mode: state.overlay_mode(),
            uptime_seconds: state.last_uptime_seconds,
            time_sync: state.time_sync,
            battery_percent: state.battery_percent,
            pattern_nonce: &mut state.pattern_nonce,
            first_visual_seed_pending: &mut state.first_visual_seed_pending,
        },
    )
    .await;
    state.screen_initialized = true;
}
