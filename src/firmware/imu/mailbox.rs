use core::sync::atomic::{AtomicBool, Ordering};

use crate::firmware::{config::APP_EVENTS, types::AppEvent};

use super::types::ImuActionSnapshot;

static BACKLIGHT_TRIGGER_PENDING: AtomicBool = AtomicBool::new(false);

pub(super) fn publish_backlight_trigger() {
    if BACKLIGHT_TRIGGER_PENDING.swap(true, Ordering::AcqRel) {
        super::metrics::record_action_coalesced();
    }
    notify_display();
}

pub(crate) fn take_pending_actions() -> ImuActionSnapshot {
    ImuActionSnapshot {
        backlight_trigger: BACKLIGHT_TRIGGER_PENDING.swap(false, Ordering::AcqRel),
    }
}

pub(crate) fn discard_pending_actions() {
    let _ = take_pending_actions();
}

fn notify_display() {
    let _ = APP_EVENTS.try_send(AppEvent::ImuActionsReady);
}
