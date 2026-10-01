//! LVGL callback ABI boundary for the typed intent bridge.
//!
//! LVGL alone calls these functions. Pointer decoding stays here; the parent
//! module receives ordinary route/index values and performs all queue and
//! lifecycle work without unsafe code.

#![allow(unsafe_code)]

use lightvgl_sys as lv;

use super::{
    enqueue, enqueue_action, enqueue_value, IntentBindings, OwnedCompositionIntent, OwnedNavIntent,
    OwnedShellIntent, OwnedUiSettingsIntent, ScreenAction, AMBIENT_TAP_REQUESTED,
};
use core::sync::atomic::Ordering;

fn route(event: *mut lv::lv_event_t) -> Option<u32> {
    if event.is_null() {
        return None;
    }
    // SAFETY: this helper is called only during an LVGL callback with LVGL's
    // live event pointer. User data is an encoded integer and is not
    // dereferenced as a Rust or C object.
    Some(unsafe { lv::lv_event_get_user_data(event) }.addr() as u32)
}

pub(crate) unsafe extern "C" fn navigation(event: *mut lv::lv_event_t) {
    #[cfg(feature = "ui-interaction-trace")]
    crate::interaction_trace::emit(26, 0, 0);
    let Some(encoded) = route(event) else {
        return;
    };
    // SAFETY: LVGL supplied the live event. The registered target is the
    // adapter-managed widget whose integer navigation index was installed by
    // `bind_navigation_click`.
    let target = unsafe { lv::lv_event_get_target_obj(event) };
    if target.is_null() {
        return;
    }
    let Some(index) = unsafe { lv::lv_obj_get_user_data(target) }
        .addr()
        .checked_sub(1)
    else {
        return;
    };
    enqueue(encoded, |bindings| match bindings {
        IntentBindings::Screen {
            source, actions, ..
        } => actions
            .get(index)
            .copied()
            .flatten()
            .map(|action| match action {
                ScreenAction::Navigate(intent) => {
                    OwnedShellIntent::Navigate(OwnedNavIntent { source, intent })
                }
                ScreenAction::Configure(intent) => {
                    OwnedShellIntent::Configure(OwnedUiSettingsIntent { source, intent })
                }
            }),
        IntentBindings::Modal { navigation, .. } if index == 0 => {
            navigation.map(OwnedShellIntent::Navigate)
        }
        IntentBindings::Modal { .. }
        | IntentBindings::Refresh { .. }
        | IntentBindings::Action { .. } => None,
    });
}

pub(crate) unsafe extern "C" fn show_confirm(event: *mut lv::lv_event_t) {
    #[cfg(feature = "ui-interaction-trace")]
    crate::interaction_trace::emit(27, 0, 0);
    let Some(encoded) = route(event) else {
        return;
    };
    enqueue(encoded, |bindings| match bindings {
        IntentBindings::Screen {
            source,
            show_confirm,
            ..
        } => Some(OwnedShellIntent::Compose(OwnedCompositionIntent {
            source,
            intent: show_confirm,
        })),
        IntentBindings::Modal { .. }
        | IntentBindings::Refresh { .. }
        | IntentBindings::Action { .. } => None,
    });
}

pub(crate) unsafe extern "C" fn dismiss_modal(event: *mut lv::lv_event_t) {
    #[cfg(feature = "ui-interaction-trace")]
    crate::interaction_trace::emit(28, 0, 0);
    let Some(encoded) = route(event) else {
        return;
    };
    enqueue(encoded, |bindings| match bindings {
        IntentBindings::Modal { dismiss, .. } => Some(OwnedShellIntent::Compose(dismiss)),
        IntentBindings::Screen { .. }
        | IntentBindings::Refresh { .. }
        | IntentBindings::Action { .. } => None,
    });
}

pub(crate) unsafe extern "C" fn full_repaint(event: *mut lv::lv_event_t) {
    let Some(encoded) = route(event) else {
        return;
    };
    enqueue(encoded, |bindings| match bindings {
        // Convert legacy refresh controls before their action enters the
        // shared queue. Consumers receive a checked, source-owned semantic
        // request instead of relying on an out-of-band repaint flag.
        IntentBindings::Refresh { request } => Some(OwnedShellIntent::ScreenUpdate(request.into())),
        IntentBindings::Screen { .. }
        | IntentBindings::Modal { .. }
        | IntentBindings::Action { .. } => None,
    });
}

pub(crate) unsafe extern "C" fn action(event: *mut lv::lv_event_t) {
    let Some(encoded) = route(event) else {
        return;
    };
    // SAFETY: LVGL supplied the live event and the target is the adapter-
    // managed widget stamped by `bind_action_click`.
    let target = unsafe { lv::lv_event_get_target_obj(event) };
    if target.is_null() {
        return;
    }
    let Some(index) = unsafe { lv::lv_obj_get_user_data(target) }
        .addr()
        .checked_sub(1)
    else {
        return;
    };
    enqueue_action(encoded, index);
}

pub(crate) unsafe extern "C" fn ambient_tap(_event: *mut lv::lv_event_t) {
    #[cfg(feature = "ui-interaction-trace")]
    crate::interaction_trace::emit(29, 0, 0);
    AMBIENT_TAP_REQUESTED.store(true, Ordering::Release);
}

pub(crate) unsafe extern "C" fn value_changed(event: *mut lv::lv_event_t) {
    let Some(encoded) = route(event) else {
        return;
    };
    // SAFETY: LVGL supplied the live event and the target is the adapter-
    // managed slider stamped by `bind_value_changed`.
    let target = unsafe { lv::lv_event_get_target_obj(event) };
    if target.is_null() {
        return;
    }
    let Some(index) = unsafe { lv::lv_obj_get_user_data(target) }
        .addr()
        .checked_sub(1)
    else {
        return;
    };
    // Capture the level inside the callback: safe code never touches LVGL,
    // so the value must be read here while LVGL owns the event.
    // SAFETY: target is the live slider LVGL delivered this value event for.
    let value = unsafe { lv::lv_slider_get_value(target) };
    enqueue_value(encoded, index, value);
}
