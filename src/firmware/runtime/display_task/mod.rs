mod app_events;
mod gpio36_feedback;
mod sd_power;
mod state;
mod touch_events;
mod wait;

use app_events::{handle_app_event, handle_pending_imu_actions};
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{with_timeout, Duration};
use gpio36_feedback::render_gpio36_ready_feedback;
use sd_power::process_sd_power_requests;
use state::DisplayLoopState;
use touch_events::process_touch_cycle;
use wait::next_loop_wait_ms;

use super::super::{
    config::{APP_EVENTS, UI_TICK_MS},
    input::gpio36::Gpio36Mode,
    touch::{tasks::request_touch_pipeline_reset, wizard::render_touch_wizard_waiting_screen},
    types::DisplayContext,
};
use super::run_backlight_timeline;

const SD_POWER_POLL_SLICE_MS: u64 = 5;
static DISPLAY_WORK_BUSY: AtomicBool = AtomicBool::new(false);

pub(crate) fn is_display_work_busy() -> bool {
    DISPLAY_WORK_BUSY.load(Ordering::Acquire)
}

#[embassy_executor::task]
pub(crate) async fn display_task(mut context: DisplayContext) {
    let mut state = DisplayLoopState::new(&mut context).await;
    process_sd_power_requests(&mut context).await;
    DISPLAY_WORK_BUSY.store(true, Ordering::Release);
    render_initial_display_state(&mut context, &mut state).await;
    process_sd_power_requests(&mut context).await;
    DISPLAY_WORK_BUSY.store(false, Ordering::Release);
    request_touch_pipeline_reset();

    loop {
        let maybe_event = receive_next_app_event_or_timeout(&mut context, &state).await;
        DISPLAY_WORK_BUSY.store(true, Ordering::Release);
        if let Some(event) = maybe_event {
            handle_app_event(event, &mut context, &mut state).await;
        }
        handle_pending_imu_actions(&mut context, &mut state).await;
        process_runtime_tasks(&mut context, &mut state).await;
        announce_runtime_ready(&mut state);
        // Service power requests before publishing the idle state. A waiter
        // that observes busy=true keeps waiting on its already-enqueued
        // request instead of creating a duplicate after a slow refresh.
        process_sd_power_requests(&mut context).await;
        DISPLAY_WORK_BUSY.store(false, Ordering::Release);
    }
}

fn announce_runtime_ready(state: &mut DisplayLoopState) {
    if state.touch_startup_settled && !state.runtime_ready_announced {
        state.runtime_ready_announced = true;
        super::scheduling::mark_runtime_ready();
        esp_println::println!("RUNTIME_READY app_state=ready display=ready");
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
    let upload_enabled = state.upload_enabled();
    // GPIO36 is a shared WAKE/touch line. Keep its classifier alive in upload
    // mode even though the rest of the interactive product UI is suspended.
    // Button-only diagnostics resolve directly from edge events and must not
    // run the touchscreen pipeline.
    if !matches!(state.gpio36_mode, Gpio36Mode::ButtonOnly) {
        process_touch_cycle(context, state).await;
    }
    if matches!(state.gpio36_mode, Gpio36Mode::ButtonOnly) && !state.gpio36_ready_announced {
        render_gpio36_ready_feedback(&mut context.inkplate).await;
        state.gpio36_ready_announced = true;
    }
    if upload_enabled {
        return;
    }
    if !state.in_touch_wizard_mode() {
        run_backlight_timeline(
            &mut context.inkplate,
            &mut state.backlight_cycle_start,
            &mut state.backlight_level,
        )
        .await;
    }
}

fn display_wait_ms(state: &DisplayLoopState) -> u64 {
    if state.upload_enabled() {
        // In upload mode IMU and product rendering are skipped. Clamp to a
        // fixed UI tick while the shared GPIO/touch classifier remains active.
        UI_TICK_MS
    } else {
        next_loop_wait_ms(state)
    }
}
