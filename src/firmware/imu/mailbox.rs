use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::firmware::{config::APP_EVENTS, types::AppEvent};

use super::types::ImuActionSnapshot;

static BACKLIGHT_TRIGGER_PENDING: AtomicBool = AtomicBool::new(false);
static DAY_BACKGROUND_TOGGLE_COUNT: AtomicU32 = AtomicU32::new(0);

pub(super) fn publish_backlight_trigger() {
    if BACKLIGHT_TRIGGER_PENDING.swap(true, Ordering::AcqRel) {
        super::metrics::record_action_coalesced();
    }
    notify_display();
}

pub(super) fn publish_day_background_toggle() {
    if DAY_BACKGROUND_TOGGLE_COUNT.fetch_add(1, Ordering::AcqRel) != 0 {
        super::metrics::record_action_coalesced();
    }
    notify_display();
}

pub(crate) fn take_pending_actions() -> ImuActionSnapshot {
    ImuActionSnapshot {
        backlight_trigger: BACKLIGHT_TRIGGER_PENDING.swap(false, Ordering::AcqRel),
        day_background_toggle_count: DAY_BACKGROUND_TOGGLE_COUNT.swap(0, Ordering::AcqRel),
    }
}

pub(crate) fn discard_pending_actions() {
    let _ = take_pending_actions();
}

fn notify_display() {
    let _ = APP_EVENTS.try_send(AppEvent::ImuActionsReady);
}
