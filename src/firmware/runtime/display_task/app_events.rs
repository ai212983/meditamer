use super::super::super::{
    app_state::AppStateCommand,
    config::APP_STATE_APPLY_ACKS,
    types::{AppEvent, AppStateApplyAck, DisplayContext, TouchStatus},
};

use super::gpio36_feedback::handle_gpio36_action;
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

include!("app_events/status_mapping.rs");
include!("app_events/dispatch.rs");
include!("app_events/lifecycle.rs");
include!("app_events/repaint.rs");
include!("app_events/apply_state.rs");
