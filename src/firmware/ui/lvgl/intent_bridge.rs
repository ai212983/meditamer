use core::{
    cell::RefCell,
    ffi::c_void,
    sync::atomic::{AtomicBool, Ordering},
};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, blocking_mutex::Mutex};
use lightvgl_sys as lv;

use shell::settings::UiSettingsIntent;
#[cfg(feature = "ui-provider-fixture")]
use shell::types::ProviderToken;
use shell::{
    callback_action_queue::CallbackActionQueue,
    callback_routes::{CallbackRoute, CallbackRouteTable},
    catalogue::CATALOGUE_CAPACITY,
    model::SHELL_INTENT_QUEUE_CAPACITY,
    types::{
        CompositionIntent, NavIntent, OwnedCompositionIntent, OwnedNavIntent, OwnedRefreshIntent,
        OwnedShellIntent, OwnedUiSettingsIntent, SurfaceInstanceToken,
    },
};

// The sticky refresh control remains live while origin + destination screens
// and departing + promoted modals coexist during one atomic handoff.
const CALLBACK_BINDING_CAPACITY: usize = 5;
pub(in crate::firmware::ui) const SCREEN_NAVIGATION_CAPACITY: usize = CATALOGUE_CAPACITY + 2;
pub(in crate::firmware::ui) const HOME_NAVIGATION_INDEX: usize = CATALOGUE_CAPACITY;
pub(in crate::firmware::ui) const BACK_NAVIGATION_INDEX: usize = CATALOGUE_CAPACITY + 1;

#[derive(Clone, Copy)]
pub(in crate::firmware::ui) enum ScreenAction {
    Navigate(NavIntent),
    Configure(UiSettingsIntent),
}

pub(in crate::firmware::ui) type CallbackRouteError = shell::callback_routes::CallbackRouteError;

#[derive(Clone, Copy)]
pub(in crate::firmware::ui) enum IntentBindings {
    Screen {
        source: SurfaceInstanceToken,
        actions: [Option<ScreenAction>; SCREEN_NAVIGATION_CAPACITY],
        show_confirm: CompositionIntent,
    },
    Modal {
        dismiss: OwnedCompositionIntent,
    },
    Refresh {
        request: OwnedRefreshIntent,
    },
}

#[cfg(feature = "ui-provider-fixture")]
impl IntentBindings {
    fn references_provider(self, owner: ProviderToken) -> bool {
        match self {
            Self::Screen {
                source,
                actions,
                show_confirm,
            } => {
                source.surface.owner == owner
                    || actions.into_iter().flatten().any(|action| {
                        match action {
                            ScreenAction::Navigate(intent) => {
                                OwnedShellIntent::Navigate(OwnedNavIntent { source, intent })
                            }
                            ScreenAction::Configure(intent) => {
                                OwnedShellIntent::Configure(OwnedUiSettingsIntent {
                                    source,
                                    intent,
                                })
                            }
                        }
                        .references_provider(owner)
                    })
                    || OwnedShellIntent::Compose(OwnedCompositionIntent {
                        source,
                        intent: show_confirm,
                    })
                    .references_provider(owner)
            }
            Self::Modal { dismiss } => {
                OwnedShellIntent::Compose(dismiss).references_provider(owner)
            }
            Self::Refresh { request } => {
                OwnedShellIntent::Refresh(request).references_provider(owner)
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::firmware::ui) struct CallbackLease {
    route: CallbackRoute,
}

impl CallbackLease {
    pub(in crate::firmware::ui) fn user_data(&self) -> *mut c_void {
        core::ptr::without_provenance_mut(self.route.encoded() as usize)
    }
}

pub(in crate::firmware::ui) fn action_user_data(index: usize) -> *mut c_void {
    core::ptr::without_provenance_mut(index + 1)
}

static BINDINGS: Mutex<
    CriticalSectionRawMutex,
    RefCell<CallbackRouteTable<IntentBindings, CALLBACK_BINDING_CAPACITY>>,
> = Mutex::new(RefCell::new(CallbackRouteTable::new()));
static CALLBACK_INTENTS: Mutex<
    CriticalSectionRawMutex,
    RefCell<CallbackActionQueue<SHELL_INTENT_QUEUE_CAPACITY>>,
> = Mutex::new(RefCell::new(CallbackActionQueue::new()));
static CALLBACK_OVERFLOWED: AtomicBool = AtomicBool::new(false);
static FULL_REPAINT_REQUESTED: AtomicBool = AtomicBool::new(false);
static AMBIENT_TAP_REQUESTED: AtomicBool = AtomicBool::new(false);

pub(in crate::firmware::ui) fn claim(
    bindings: IntentBindings,
) -> Result<CallbackLease, CallbackRouteError> {
    BINDINGS.lock(|routes| {
        routes
            .borrow_mut()
            .claim(bindings)
            .map(|route| CallbackLease { route })
    })
}

pub(in crate::firmware::ui) fn enable(lease: &CallbackLease) -> Result<(), CallbackRouteError> {
    BINDINGS.lock(|routes| routes.borrow_mut().enable(lease.route))
}

pub(in crate::firmware::ui) fn disable(lease: &CallbackLease) -> Result<(), CallbackRouteError> {
    BINDINGS.lock(|routes| routes.borrow_mut().disable(lease.route))
}

pub(in crate::firmware::ui) fn release(lease: &CallbackLease) -> Result<(), CallbackRouteError> {
    BINDINGS.lock(|routes| routes.borrow_mut().release(lease.route))
}

pub(in crate::firmware::ui) fn take_intent() -> Option<OwnedShellIntent> {
    CALLBACK_INTENTS.lock(|intents| intents.borrow_mut().pop())
}

pub(in crate::firmware::ui) fn purge_instance(source: SurfaceInstanceToken) -> usize {
    CALLBACK_INTENTS.lock(|intents| intents.borrow_mut().purge_instance(source))
}

#[cfg(feature = "ui-provider-fixture")]
pub(in crate::firmware::ui) fn purge_provider(owner: ProviderToken) -> usize {
    CALLBACK_INTENTS.lock(|intents| intents.borrow_mut().purge_provider(owner))
}

#[cfg(feature = "ui-provider-fixture")]
pub(in crate::firmware::ui) fn queued_provider_action_count(owner: ProviderToken) -> usize {
    CALLBACK_INTENTS.lock(|intents| intents.borrow().provider_reference_count(owner))
}

#[cfg(feature = "ui-provider-fixture")]
pub(in crate::firmware::ui) fn references_provider(owner: ProviderToken) -> bool {
    CALLBACK_INTENTS.lock(|intents| intents.borrow().references_provider(owner))
        || BINDINGS.lock(|routes| {
            routes
                .borrow()
                .any_value(|bindings| bindings.references_provider(owner))
        })
}

pub(in crate::firmware::ui) fn take_overflowed() -> bool {
    CALLBACK_OVERFLOWED.swap(false, Ordering::AcqRel)
}

pub(crate) fn take_full_repaint_request() -> bool {
    FULL_REPAINT_REQUESTED.swap(false, Ordering::AcqRel)
}

pub(in crate::firmware::ui) fn mark_full_repaint_requested() {
    FULL_REPAINT_REQUESTED.store(true, Ordering::Release);
}

/// Consumes a pending Ambient Home background tap, if any. The Ambient Home
/// screen's own poll loop is responsible for discarding a stale flag left
/// over from a screen that is no longer active.
pub(in crate::firmware::ui) fn take_ambient_tap_requested() -> bool {
    AMBIENT_TAP_REQUESTED.swap(false, Ordering::AcqRel)
}

fn enqueue(
    event: *mut lv::lv_event_t,
    select: impl FnOnce(IntentBindings) -> Option<OwnedShellIntent>,
) {
    if event.is_null() {
        return;
    }
    let encoded = unsafe { lv::lv_event_get_user_data(event) }.addr() as u32;
    let Some(route) = CallbackRoute::from_encoded(encoded) else {
        return;
    };
    let intent = BINDINGS.lock(|routes| routes.borrow().resolve(route).and_then(select));
    let Some(intent) = intent else {
        return;
    };
    CALLBACK_INTENTS.lock(|intents| {
        if intents.borrow_mut().push(intent).is_err() {
            CALLBACK_OVERFLOWED.store(true, Ordering::Release);
        }
    });
}

pub(in crate::firmware::ui) unsafe extern "C" fn navigation_callback(event: *mut lv::lv_event_t) {
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
    enqueue(event, |bindings| match bindings {
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
        IntentBindings::Modal { .. } | IntentBindings::Refresh { .. } => None,
    });
}

pub(in crate::firmware::ui) unsafe extern "C" fn show_confirm_callback(event: *mut lv::lv_event_t) {
    enqueue(event, |bindings| match bindings {
        IntentBindings::Screen {
            source,
            show_confirm,
            ..
        } => Some(OwnedShellIntent::Compose(OwnedCompositionIntent {
            source,
            intent: show_confirm,
        })),
        IntentBindings::Modal { .. } | IntentBindings::Refresh { .. } => None,
    });
}

pub(in crate::firmware::ui) unsafe extern "C" fn dismiss_modal_callback(
    event: *mut lv::lv_event_t,
) {
    enqueue(event, |bindings| match bindings {
        IntentBindings::Modal { dismiss } => Some(OwnedShellIntent::Compose(dismiss)),
        IntentBindings::Screen { .. } | IntentBindings::Refresh { .. } => None,
    });
}

pub(in crate::firmware::ui) unsafe extern "C" fn full_repaint_callback(event: *mut lv::lv_event_t) {
    enqueue(event, |bindings| match bindings {
        IntentBindings::Refresh { request } => Some(OwnedShellIntent::Refresh(request)),
        IntentBindings::Screen { .. } | IntentBindings::Modal { .. } => None,
    });
}

/// Ambient Home's background-tap signal. Unlike the callbacks above, this
/// carries no per-instance payload and needs no shell-level navigation or
/// overlay side effect, so it bypasses the `IntentBindings`/route/queue
/// machinery entirely and just flags an atomic -- the same "flag it
/// happened" idiom as [`mark_full_repaint_requested`]/
/// [`take_full_repaint_request`], just set directly from the LVGL callback
/// instead of via a routed intent.
pub(in crate::firmware::ui) unsafe extern "C" fn ambient_tap_callback(_event: *mut lv::lv_event_t) {
    AMBIENT_TAP_REQUESTED.store(true, Ordering::Release);
}
