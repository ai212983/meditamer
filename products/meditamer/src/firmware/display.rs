mod app_events;
mod battery_delivery;
#[cfg(feature = "cpu-load")]
mod cpu_delivery;
mod environment_delivery;
mod frontlight;
mod frontlight_policy;
mod gpio36_feedback;
use observation::fixture as observation_fixture;
mod panel;
mod presentation;
mod scheduling;
mod state;
mod wall_clock;
mod work_wait;

pub use panel::refresh::full_refresh_panel_gray4;

use super::{
    input::gpio36::Gpio36Mode, touch::tasks::request_touch_pipeline_reset, types::DisplayContext,
};
use app_events::{handle_app_event, handle_pending_imu_actions};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use frontlight::run_backlight_timeline;
use frontlight_policy::{check_hold, HoldOutcome};
use panel::sd_power::process_sd_power_requests;
use state::DisplayLoopState;

static DISPLAY_WAKE: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

pub(crate) fn wake() {
    DISPLAY_WAKE.signal(());
}

static DISPLAY_WORK_BUSY: AtomicBool = AtomicBool::new(false);

// Temporary thermal experiment control. Nonpersistent; expires after one hour.
static TIMER_PAUSE_UNTIL_SECONDS: AtomicU32 = AtomicU32::new(0);

pub(crate) fn set_timer_pause(paused: bool) {
    let until = if paused {
        (embassy_time::Instant::now().as_secs() as u32).saturating_add(3_600)
    } else {
        0
    };
    TIMER_PAUSE_UNTIL_SECONDS.store(until, Ordering::Release);
    wake();
}

pub(crate) fn timer_paused() -> bool {
    (embassy_time::Instant::now().as_secs() as u32)
        < TIMER_PAUSE_UNTIL_SECONDS.load(Ordering::Acquire)
}

pub(crate) fn is_display_work_busy() -> bool {
    DISPLAY_WORK_BUSY.load(Ordering::Acquire)
}

#[embassy_executor::task]
pub async fn display_task(mut context: DisplayContext) {
    let mut state = match DisplayLoopState::new(&mut context).await {
        Ok(state) => state,
        Err(error) => {
            console::println!(
                "RUNTIME_READY blocked=display_state_allocation error={:?}",
                error
            );
            crate::firmware::reset_pending_update_or_halt();
        }
    };
    battery_delivery::start_battery(&mut state);
    process_sd_power_requests(&mut context).await;
    DISPLAY_WORK_BUSY.store(true, Ordering::Release);
    render_initial_display_state(&mut context, &mut state).await;
    process_sd_power_requests(&mut context).await;
    DISPLAY_WORK_BUSY.store(false, Ordering::Release);
    request_touch_pipeline_reset();

    loop {
        let maybe_event = scheduling::wait(&state).await;
        // Clear before inspecting durable state. Any later publication remains latched.
        DISPLAY_WAKE.reset();
        DISPLAY_WORK_BUSY.store(true, Ordering::Release);
        process_sd_power_requests(&mut context).await;
        if let Some(event) = maybe_event {
            handle_app_event(event, &mut context, &mut state).await;
        }
        handle_pending_imu_actions(&mut context, &mut state).await;
        environment_delivery::poll_environment(&mut state);
        battery_delivery::poll_battery(&mut state);
        #[cfg(feature = "cpu-load")]
        cpu_delivery::poll_cpu(&mut state);
        // Stack-overflow hunt: track the display/service loop's low-water
        // against the main-task guard (sensor delivery and event handling
        // run here on the main executor).
        crate::firmware::observability::record_stack_headroom();
        process_runtime_tasks(&mut context, &mut state).await;
        environment_delivery::reconcile_environment_owner(&mut state);
        announce_runtime_ready(&mut state);
        // Service power requests before publishing the idle state. A waiter
        // that observes busy=true keeps waiting on its already-enqueued
        // request instead of creating a duplicate after a slow refresh.
        process_sd_power_requests(&mut context).await;
        DISPLAY_WORK_BUSY.store(false, Ordering::Release);
    }
}

fn announce_runtime_ready(state: &mut DisplayLoopState) {
    if state.touch_startup_settled
        && state.presentation.is_ready()
        && !state.runtime_ready_announced
    {
        state.runtime_ready_announced = true;
        crate::firmware::scheduling::mark_runtime_ready();
        console::println!("RUNTIME_READY app_state=ready display=ready");
    }
}

async fn render_initial_display_state(context: &mut DisplayContext, state: &mut DisplayLoopState) {
    if !presentation::initialize(context, &mut state.presentation).await {
        console::println!("RUNTIME_READY blocked=display_init_failed");
    }
}

async fn process_runtime_tasks(context: &mut DisplayContext, state: &mut DisplayLoopState) {
    // Rail cleanup must run before readiness and input-mode gates.
    panel::refresh::service_panel_power_lease(context, &mut state.presentation).await;
    // GPIO36 is a shared WAKE/touch line. Keep touch and LVGL serviced during
    // uploads so the bounded event queue cannot back up. Upload enablement
    // controls service availability, not presentation or frontlight admission.
    // Button-only diagnostics resolve directly from edge events and therefore
    // do not run the touchscreen pipeline.
    if !matches!(state.gpio36_mode, Gpio36Mode::ButtonOnly) {
        presentation::process_cycle(context, &mut state.presentation).await;
    }
    if frontlight::service_calibration(context, state).await {
        presentation::finish_frontlight_effect_refresh(context, &mut state.presentation).await;
    }
    service_wake_hold(context, state).await;
    let steady_level = state
        .frontlight_calibration
        .map_or(state.frontlight_baseline, |session| session.preview);
    run_backlight_timeline(
        &mut context.inkplate,
        &mut state.backlight_cycle_start,
        &mut state.backlight_level,
        steady_level,
        &mut state.backlight_retry_at_ms,
    )
    .await;
}

/// Display-side (core 0) long-press decision for the armed WAKE hold. The
/// acquisition task only publishes edges and the snapshot; calibration
/// happens here, at most once per hold, against the source press timestamp
/// so delayed servicing still decides the true hold duration.
async fn service_wake_hold(context: &mut DisplayContext, state: &mut DisplayLoopState) {
    let now_ms = embassy_time::Instant::now().as_millis();
    let snapshot = super::input::gpio36::load_wake_snapshot();
    match check_hold(
        state.wake_press,
        now_ms,
        snapshot.generation,
        snapshot.active,
    ) {
        HoldOutcome::Wait => {}
        HoldOutcome::DropStale => {
            console::println!("input: gpio36 wake_hold state=dropped reason=stale");
            state.wake_press = None;
        }
        HoldOutcome::OpenCalibration => {
            if let Some(press) = state.wake_press.as_mut() {
                press.long_fired = true;
            }
            let opened = frontlight::open_calibration(context, state).await;
            console::println!(
                "input: gpio36 wake_hold state=long calibration={}",
                if opened { "opened" } else { "rejected" },
            );
        }
    }
}
