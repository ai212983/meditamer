//! Wait on durable work queues and the earliest owner deadline. Queue readiness
//! does not consume touch frames, preserving the existing timestamp merge/reset order.
use super::{state::DisplayLoopState, DISPLAY_WAKE};
use crate::firmware::{
    config::{APP_EVENTS, SD_POWER_REQUESTS},
    touch::config::{TOUCH_LVGL_MULTITOUCH_FRAMES, TOUCH_PIPELINE_EVENTS},
    types::AppEvent,
};
use embassy_time::{Instant, Timer};

fn deadline(state: &DisplayLoopState) -> Option<u64> {
    let now = Instant::now().as_millis();
    let p = &state.presentation;
    let mut next = None;
    let mut include = |at: Option<u64>| {
        if let Some(at) = at {
            next = Some(next.map_or(at, |old: u64| old.min(at)));
        }
    };
    include(p.panel_power_lease.next_deadline_ms());
    include(p.refresh_tracking.next_deadline_ms());
    include(p.touch_equivalence.next_deadline_ms());
    include(super::frontlight::next_deadline_ms(state, now));
    // The WAKE hold deadline derives from the source press timestamp, so a
    // delayed display pass still decides the true hold duration. A past
    // deadline wakes immediately via `Timer::at`.
    let wake_snapshot = crate::firmware::input::gpio36::load_wake_snapshot();
    let wake_deadline = state.wake_press.and_then(|press| {
        if wake_snapshot.active && wake_snapshot.generation == press.generation {
            super::frontlight_policy::hold_deadline_ms(Some(press))
        } else {
            None
        }
    });
    include(wake_deadline);
    if state.environment_delivery.needs_service_retry()
        || state.battery_delivery.needs_service_retry()
    {
        include(Some(now.saturating_add(8)));
    }
    if super::timer_paused() {
        include(Some(
            u64::from(super::TIMER_PAUSE_UNTIL_SECONDS.load(core::sync::atomic::Ordering::Acquire))
                * 1000,
        ));
    } else if !p.refresh_tracking.recovery_required() {
        include(p.backend.as_ref().map(|b| b.next_service_ms(now)));
        if p.gesture_page_refresh_pending {
            include(Some(now.saturating_add(8)));
        }
    }
    next
}

pub(super) async fn wait(state: &DisplayLoopState) -> Option<AppEvent> {
    let at = deadline(state);
    let timer = async {
        match at {
            Some(at) => Timer::at(Instant::from_millis(at)).await,
            None => core::future::pending::<()>().await,
        }
    };
    // Recovery must run before touch intake. Do not repeatedly wake on a queue
    // that this pass cannot drain; recovery's own deadline releases that gate.
    let include_touch = !state.presentation.refresh_tracking.recovery_required()
        && !matches!(
            state.gpio36_mode,
            crate::firmware::input::gpio36::Gpio36Mode::ButtonOnly
        );
    super::work_wait::wait(
        &APP_EVENTS,
        &SD_POWER_REQUESTS,
        &TOUCH_PIPELINE_EVENTS,
        &TOUCH_LVGL_MULTITOUCH_FRAMES,
        include_touch,
        &DISPLAY_WAKE,
        timer,
    )
    .await
}
