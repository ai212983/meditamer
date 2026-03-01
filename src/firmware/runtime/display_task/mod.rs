mod app_events;
mod imu;
mod sd_power;
mod state;
mod touch_loop;
mod wait;

use app_events::handle_app_event;
use embassy_time::{with_timeout, Duration};
use imu::process_imu_cycle;
use sd_power::process_sd_power_requests;
use state::DisplayLoopState;
use touch_loop::process_touch_cycle;
use wait::next_loop_wait_ms;

use super::super::{
    config::{APP_EVENTS, UI_TICK_MS},
    touch::{tasks::request_touch_pipeline_reset, wizard::render_touch_wizard_waiting_screen},
    types::DisplayContext,
};
use super::run_backlight_timeline;

const SD_POWER_POLL_SLICE_MS: u64 = 5;

#[embassy_executor::task]
pub(crate) async fn display_task(mut context: DisplayContext) {
    let mut state = DisplayLoopState::new(&mut context).await;
    process_sd_power_requests(&mut context).await;
    render_initial_display_state(&mut context, &mut state).await;
    process_sd_power_requests(&mut context).await;
    request_touch_pipeline_reset();

    loop {
        let maybe_event = receive_next_app_event_or_timeout(&mut context, &state).await;
        if let Some(event) = maybe_event {
            handle_app_event(event, &mut context, &mut state).await;
        }
        process_runtime_tasks(&mut context, &mut state).await;
    }
}

async fn render_initial_display_state(context: &mut DisplayContext, state: &mut DisplayLoopState) {
    if state.in_touch_wizard_mode() && state.touch_wizard.is_active() {
        state.touch_wizard.render_full(&mut context.inkplate).await;
        state.screen_initialized = true;
    } else if state.in_touch_wizard_mode() {
        render_touch_wizard_waiting_screen(&mut context.inkplate).await;
        state.screen_initialized = true;
    }
}

async fn receive_next_app_event_or_timeout(
    context: &mut DisplayContext,
    state: &DisplayLoopState,
) -> Option<crate::firmware::types::AppEvent> {
    let mut remaining_wait_ms = display_wait_ms(state);
    let mut event = None;
    while remaining_wait_ms > 0 {
        process_sd_power_requests(context).await;
        let wait_slice_ms = remaining_wait_ms.min(SD_POWER_POLL_SLICE_MS);
        if let Ok(received_event) =
            with_timeout(Duration::from_millis(wait_slice_ms), APP_EVENTS.receive()).await
        {
            event = Some(received_event);
            break;
        }
        remaining_wait_ms = remaining_wait_ms.saturating_sub(wait_slice_ms);
    }
    event
}

async fn process_runtime_tasks(context: &mut DisplayContext, state: &mut DisplayLoopState) {
    if state.upload_enabled() {
        return;
    }
    process_imu_cycle(context, state).await;
    process_touch_cycle(context, state).await;
    if !state.in_touch_wizard_mode() {
        run_backlight_timeline(
            &mut context.inkplate,
            &mut state.backlight_cycle_start,
            &mut state.backlight_level,
        );
    }
}

fn display_wait_ms(state: &DisplayLoopState) -> u64 {
    if state.upload_enabled() {
        // In upload mode touch/IMU loops are skipped, so their deadlines can go stale.
        // Clamp to a fixed UI tick to avoid a busy-looping zero-wait path.
        UI_TICK_MS
    } else {
        next_loop_wait_ms(state)
    }
}
