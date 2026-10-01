//! App-event handling for the display task.

use super::super::types::DisplayContext;

use super::state::DisplayLoopState;

pub(super) async fn handle_pending_imu_actions(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    let actions = super::super::imu::take_pending_actions();
    if actions.backlight_trigger {
        super::frontlight::trigger_backlight_cycle(
            &mut context.inkplate,
            &mut state.backlight_cycle_start,
            &mut state.backlight_level,
            &mut state.backlight_retry_at_ms,
        )
        .await;
    }
}

mod apply_state;
mod dispatch;
mod lifecycle;
mod panel_fixture;
mod repaint;
mod status_mapping;
mod ui_cycle;

pub(super) use dispatch::handle_app_event;
