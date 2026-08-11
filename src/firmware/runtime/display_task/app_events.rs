//! App-event handling for the display task.

use super::super::super::types::DisplayContext;

use super::state::DisplayLoopState;

pub(super) async fn handle_pending_imu_actions(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    if state.upload_enabled() {
        super::super::super::imu::discard_pending_actions();
        return;
    }

    let actions = super::super::super::imu::take_pending_actions();
    if actions.backlight_trigger {
        super::super::trigger_backlight_cycle(
            &mut context.inkplate,
            &mut state.backlight_cycle_start,
            &mut state.backlight_level,
        )
        .await;
    }
}

mod apply_state;
mod dispatch;
mod lifecycle;
mod repaint;
mod status_mapping;
mod ui_cycle;

pub(super) use dispatch::handle_app_event;
