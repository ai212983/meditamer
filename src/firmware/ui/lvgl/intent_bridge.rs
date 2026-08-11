use core::{
    cell::RefCell,
    ffi::c_void,
    sync::atomic::{AtomicBool, Ordering},
};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, blocking_mutex::Mutex};
use lightvgl_sys as lv;

#[cfg(feature = "ui-provider-fixture")]
use crate::firmware::ui::shell::types::ProviderToken;
use crate::firmware::ui::shell::{
    callback_action_queue::CallbackActionQueue,
    callback_routes::{CallbackRoute, CallbackRouteTable},
    model::SHELL_INTENT_QUEUE_CAPACITY,
    types::{
        OwnedCompositionIntent, OwnedNavIntent, OwnedRefreshIntent, OwnedShellIntent,
        SurfaceInstanceToken,
    },
};

// The sticky refresh control remains live while origin + destination screens
// and departing + promoted modals coexist during one atomic handoff.
const CALLBACK_BINDING_CAPACITY: usize = 5;

pub(super) type CallbackRouteError =
    crate::firmware::ui::shell::callback_routes::CallbackRouteError;

#[derive(Clone, Copy)]
pub(super) enum IntentBindings {
    Screen {
        open_launcher: OwnedNavIntent,
        launch_diagnostics: OwnedNavIntent,
        home: OwnedNavIntent,
        show_confirm: OwnedCompositionIntent,
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
                open_launcher,
                launch_diagnostics,
                home,
                show_confirm,
            } => {
                OwnedShellIntent::Navigate(open_launcher).references_provider(owner)
                    || OwnedShellIntent::Navigate(launch_diagnostics).references_provider(owner)
                    || OwnedShellIntent::Navigate(home).references_provider(owner)
                    || OwnedShellIntent::Compose(show_confirm).references_provider(owner)
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
pub(super) struct CallbackLease {
    route: CallbackRoute,
}

impl CallbackLease {
    pub(super) fn user_data(&self) -> *mut c_void {
        self.route.encoded() as usize as *mut c_void
    }
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

pub(super) fn claim(bindings: IntentBindings) -> Result<CallbackLease, CallbackRouteError> {
    BINDINGS.lock(|routes| {
        routes
            .borrow_mut()
            .claim(bindings)
            .map(|route| CallbackLease { route })
    })
}

pub(super) fn enable(lease: &CallbackLease) -> Result<(), CallbackRouteError> {
    BINDINGS.lock(|routes| routes.borrow_mut().enable(lease.route))
}

pub(super) fn disable(lease: &CallbackLease) -> Result<(), CallbackRouteError> {
    BINDINGS.lock(|routes| routes.borrow_mut().disable(lease.route))
}

pub(super) fn release(lease: &CallbackLease) -> Result<(), CallbackRouteError> {
    BINDINGS.lock(|routes| routes.borrow_mut().release(lease.route))
}

pub(super) fn take_intent() -> Option<OwnedShellIntent> {
    CALLBACK_INTENTS.lock(|intents| intents.borrow_mut().pop())
}

pub(super) fn purge_instance(source: SurfaceInstanceToken) -> usize {
    CALLBACK_INTENTS.lock(|intents| intents.borrow_mut().purge_instance(source))
}

#[cfg(feature = "ui-provider-fixture")]
pub(super) fn purge_provider(owner: ProviderToken) -> usize {
    CALLBACK_INTENTS.lock(|intents| intents.borrow_mut().purge_provider(owner))
}

#[cfg(feature = "ui-provider-fixture")]
pub(super) fn queued_provider_action_count(owner: ProviderToken) -> usize {
    CALLBACK_INTENTS.lock(|intents| intents.borrow().provider_reference_count(owner))
}

#[cfg(feature = "ui-provider-fixture")]
pub(super) fn references_provider(owner: ProviderToken) -> bool {
    CALLBACK_INTENTS.lock(|intents| intents.borrow().references_provider(owner))
        || BINDINGS.lock(|routes| {
            routes
                .borrow()
                .any_value(|bindings| bindings.references_provider(owner))
        })
}

pub(super) fn take_overflowed() -> bool {
    CALLBACK_OVERFLOWED.swap(false, Ordering::AcqRel)
}

pub(crate) fn take_full_repaint_request() -> bool {
    FULL_REPAINT_REQUESTED.swap(false, Ordering::AcqRel)
}

pub(super) fn mark_full_repaint_requested() {
    FULL_REPAINT_REQUESTED.store(true, Ordering::Release);
}

fn enqueue(
    event: *mut lv::lv_event_t,
    select: impl FnOnce(IntentBindings) -> Option<OwnedShellIntent>,
) {
    if event.is_null() {
        return;
    }
    let encoded = unsafe { lv::lv_event_get_user_data(event) } as usize as u32;
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

pub(super) unsafe extern "C" fn open_launcher_callback(event: *mut lv::lv_event_t) {
    enqueue(event, |bindings| match bindings {
        IntentBindings::Screen { open_launcher, .. } => {
            Some(OwnedShellIntent::Navigate(open_launcher))
        }
        IntentBindings::Modal { .. } | IntentBindings::Refresh { .. } => None,
    });
}

pub(super) unsafe extern "C" fn launch_diagnostics_callback(event: *mut lv::lv_event_t) {
    enqueue(event, |bindings| match bindings {
        IntentBindings::Screen {
            launch_diagnostics, ..
        } => Some(OwnedShellIntent::Navigate(launch_diagnostics)),
        IntentBindings::Modal { .. } | IntentBindings::Refresh { .. } => None,
    });
}

pub(super) unsafe extern "C" fn home_callback(event: *mut lv::lv_event_t) {
    enqueue(event, |bindings| match bindings {
        IntentBindings::Screen { home, .. } => Some(OwnedShellIntent::Navigate(home)),
        IntentBindings::Modal { .. } | IntentBindings::Refresh { .. } => None,
    });
}

pub(super) unsafe extern "C" fn show_confirm_callback(event: *mut lv::lv_event_t) {
    enqueue(event, |bindings| match bindings {
        IntentBindings::Screen { show_confirm, .. } => {
            Some(OwnedShellIntent::Compose(show_confirm))
        }
        IntentBindings::Modal { .. } | IntentBindings::Refresh { .. } => None,
    });
}

pub(super) unsafe extern "C" fn dismiss_modal_callback(event: *mut lv::lv_event_t) {
    enqueue(event, |bindings| match bindings {
        IntentBindings::Modal { dismiss } => Some(OwnedShellIntent::Compose(dismiss)),
        IntentBindings::Screen { .. } | IntentBindings::Refresh { .. } => None,
    });
}

pub(super) unsafe extern "C" fn full_repaint_callback(event: *mut lv::lv_event_t) {
    enqueue(event, |bindings| match bindings {
        IntentBindings::Refresh { request } => Some(OwnedShellIntent::Refresh(request)),
        IntentBindings::Screen { .. } | IntentBindings::Modal { .. } => None,
    });
}
