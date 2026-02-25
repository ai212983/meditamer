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
    config::APP_EVENTS,
    touch::{tasks::request_touch_pipeline_reset, wizard::render_touch_wizard_waiting_screen},
    types::DisplayContext,
};
use super::run_backlight_timeline;

const SD_POWER_POLL_SLICE_MS: u64 = 5;

#[embassy_executor::task]
pub(crate) async fn display_task(mut context: DisplayContext) {
    let mut state = DisplayLoopState::new(&mut context);

    if state.touch_wizard.is_active() {
        state.touch_wizard.render_full(&mut context.inkplate).await;
        state.screen_initialized = true;
    } else if state.touch_wizard_requested {
        render_touch_wizard_waiting_screen(&mut context.inkplate).await;
        state.screen_initialized = true;
    }
    request_touch_pipeline_reset();

    loop {
        let app_wait_ms = next_loop_wait_ms(&state);

        let mut event = None;
        let mut remaining_wait_ms = app_wait_ms;
        loop {
            process_sd_power_requests(&mut context).await;

            if remaining_wait_ms == 0 {
                break;
            }
            let wait_slice_ms = remaining_wait_ms.min(SD_POWER_POLL_SLICE_MS);
            match with_timeout(Duration::from_millis(wait_slice_ms), APP_EVENTS.receive()).await {
                Ok(received_event) => {
                    event = Some(received_event);
                    break;
                }
                Err(_) => {
                    remaining_wait_ms = remaining_wait_ms.saturating_sub(wait_slice_ms);
                }
            }
        }

        if let Some(event) = event {
            handle_app_event(event, &mut context, &mut state).await;
        }

        process_imu_cycle(&mut context, &mut state).await;
        process_touch_cycle(&mut context, &mut state).await;

        if !state.touch_wizard_requested {
            run_backlight_timeline(
                &mut context.inkplate,
                &mut state.backlight_cycle_start,
                &mut state.backlight_level,
            );
        }
    }
}
