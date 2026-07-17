use embassy_time::Instant;

use super::super::super::{
    app_state::{AppStateCommand, BaseMode, OverlayMode},
    config::{APP_STATE_APPLY_ACKS, FULL_REFRESH_EVERY_N_UPDATES},
    render::{
        next_visual_seed, render_active_mode, render_clock_overlay, render_shanshui_update,
        render_suminagashi_update, render_visual_update, sample_battery_percent,
        RenderActiveParams, RenderVisualParams,
    },
    touch::{
        tasks::request_touch_pipeline_reset,
        wizard::{render_touch_wizard_waiting_screen, TouchCalibrationWizard},
    },
    types::{AppEvent, AppStateApplyAck, DisplayContext, TimeSyncState, TouchStatus},
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
    if actions.backlight_trigger && !state.in_touch_wizard_mode() {
        super::super::trigger_backlight_cycle(
            &mut context.inkplate,
            &mut state.backlight_cycle_start,
            &mut state.backlight_level,
        )
        .await;
    }

    if actions.day_background_toggle_count & 1 != 0 {
        let result = state
            .apply_state_command(context, AppStateCommand::ToggleDayBackground)
            .await;
        if result.changed() && !state.in_touch_wizard_mode() {
            state.update_count = 0;
            render_active_mode_from_state(context, state, state.last_uptime_seconds).await;
            state.screen_initialized = true;
        }
    }
}

include!("app_events/status_mapping.rs");
include!("app_events/dispatch.rs");
include!("app_events/render_helpers.rs");
include!("app_events/lifecycle.rs");
#[cfg(feature = "wifi-debug-slim-app")]
include!("app_events/touch_wizard_stub.rs");
#[cfg(not(feature = "wifi-debug-slim-app"))]
include!("app_events/touch_wizard.rs");
include!("app_events/repaint.rs");
include!("app_events/apply_state.rs");
