use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::lvgl_adapter::{UiAccessToken, Widget, WidgetCallbackError};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, blocking_mutex::Mutex};
use heapless::Deque;
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

pub(crate) mod callbacks;

// The sticky refresh control remains live while origin + destination screens
// and departing + promoted modals coexist during one atomic handoff.
const CALLBACK_BINDING_CAPACITY: usize = 5;
pub const SCREEN_NAVIGATION_CAPACITY: usize = CATALOGUE_CAPACITY + 2;
pub const HOME_NAVIGATION_INDEX: usize = CATALOGUE_CAPACITY;
pub const BACK_NAVIGATION_INDEX: usize = CATALOGUE_CAPACITY + 1;

#[derive(Clone, Copy)]
pub enum ScreenAction {
    Navigate(NavIntent),
    Configure(UiSettingsIntent),
}

pub type CallbackRouteError = shell::callback_routes::CallbackRouteError;

#[derive(Clone, Copy)]
pub enum IntentBindings {
    Screen {
        source: SurfaceInstanceToken,
        actions: [Option<ScreenAction>; SCREEN_NAVIGATION_CAPACITY],
        show_confirm: CompositionIntent,
    },
    Modal {
        dismiss: OwnedCompositionIntent,
        navigation: Option<OwnedNavIntent>,
    },
    Refresh {
        request: OwnedRefreshIntent,
    },
    /// Product-neutral indexed actions owned by one concrete surface
    /// instance. The product interprets the index only after this bridge has
    /// generation-checked the callback route and source. Slider value events
    /// resolve this same lease (see [`RouteCallback::Value`]): clicks queue
    /// reliably per press while values coalesce into the single latest-value
    /// slot consumed through [`take_value_action`], so one lease covers the
    /// overlay's clicks and drags alike.
    Action {
        source: SurfaceInstanceToken,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexedAction {
    pub source: SurfaceInstanceToken,
    pub index: usize,
}

/// A coalesced slider level: the latest value observed for the pending
/// (route, index) owner. The product interprets the index only after this
/// bridge has generation-checked the callback route and source, exactly as
/// for [`IndexedAction`]. Production supports one active slider, so the
/// bridge holds a single such level (see `CALLBACK_VALUE`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexedValueAction {
    pub source: SurfaceInstanceToken,
    pub index: usize,
    pub value: i32,
}

#[derive(Clone, Copy)]
struct ValueSlot {
    route: CallbackRoute,
    action: IndexedValueAction,
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
            Self::Modal {
                dismiss,
                navigation,
            } => {
                OwnedShellIntent::Compose(dismiss).references_provider(owner)
                    || navigation.is_some_and(|intent| {
                        OwnedShellIntent::Navigate(intent).references_provider(owner)
                    })
            }
            Self::Refresh { request } => {
                OwnedShellIntent::ScreenUpdate(request.into()).references_provider(owner)
            }
            Self::Action { source } => source.surface.owner == owner,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct CallbackLease {
    route: CallbackRoute,
}

impl CallbackLease {
    fn encoded(&self) -> u32 {
        self.route.encoded()
    }

    /// Opaque route identity for diagnostics that must replay an event after
    /// the original lease has been released. Resolution still goes through
    /// the route table, so a stale identity cannot deliver an intent.
    pub fn identity(&self) -> CallbackIdentity {
        CallbackIdentity { route: self.route }
    }
}

#[derive(Clone, Copy)]
pub struct CallbackIdentity {
    route: CallbackRoute,
}

/// A typed click route; product code never constructs a callback or payload
/// pointer and the adapter keeps each callback paired with its route format.
#[derive(Clone, Copy)]
pub enum RouteCallback {
    /// Routes through [`navigation_callback`], which reads its button index
    /// back off the widget's own user data rather than off the route --
    /// `bind_click` stamps it there via [`Widget::set_navigation_index`], so
    /// callers never call that separately.
    Navigation { index: usize },
    /// Routes through [`show_confirm_callback`].
    ShowConfirm,
    /// Routes through [`dismiss_modal_callback`].
    DismissModal,
    /// Routes through [`full_repaint_callback`].
    FullRepaint,
    /// Routes a product-neutral action index through [`take_action`].
    Action { index: usize },
    /// Routes a slider value through [`take_value_action`]'s single
    /// latest-value slot. The callback captures the slider's own LVGL value
    /// plus the stamped index; `bind_click` stamps the index into the same
    /// user-data slot [`RouteCallback::Action`] uses, so a widget carrying
    /// both bindings must use the same index for both. This resolves an
    /// [`IntentBindings::Action`] lease -- there is no value-specific lease,
    /// so the overlay keeps one callback lease for clicks and drags alike.
    Value { index: usize },
}

/// Binds `widget`'s click event to one of this bridge's typed routes --
/// the typed equivalent of calling `Widget::on_click` by hand with one of
/// this module's raw callback functions and a [`CallbackLease::user_data`].
pub fn bind_click(
    widget: &Widget,
    token: &UiAccessToken,
    callback: RouteCallback,
    lease: &CallbackLease,
) -> Result<(), WidgetCallbackError> {
    match callback {
        RouteCallback::Navigation { index } => {
            widget.bind_navigation_click(token, index, lease.encoded())
        }
        RouteCallback::ShowConfirm => widget.bind_show_confirm_click(token, lease.encoded()),
        RouteCallback::DismissModal => widget.bind_dismiss_modal_click(token, lease.encoded()),
        RouteCallback::FullRepaint => widget.bind_full_repaint_click(token, lease.encoded()),
        RouteCallback::Action { index } => widget.bind_action_click(token, index, lease.encoded()),
        RouteCallback::Value { index } => widget.bind_value_changed(token, index, lease.encoded()),
    }
}

/// Binds a diagnostic navigation click using an opaque route identity. This
/// is intentionally separate from [`bind_click`]: production widgets should
/// retain their lease, while lifecycle fixtures use this to prove a released
/// route cannot be resurrected after teardown or LVGL reinitialization.
pub fn bind_navigation_identity(
    widget: &Widget,
    token: &UiAccessToken,
    identity: CallbackIdentity,
    index: usize,
) -> Result<(), WidgetCallbackError> {
    widget.bind_navigation_click(token, index, identity.route.encoded())
}

/// Binds `widget`'s click event to [`ambient_tap_callback`] -- Ambient
/// Home's background-tap signal, which (see that callback's own doc)
/// carries no per-instance payload and so needs no [`CallbackLease`], unlike
/// every route [`bind_click`] installs.
pub fn bind_ambient_tap(widget: &Widget, token: &UiAccessToken) -> Result<(), WidgetCallbackError> {
    widget.bind_ambient_tap(token)
}

static BINDINGS: Mutex<
    CriticalSectionRawMutex,
    RefCell<CallbackRouteTable<IntentBindings, CALLBACK_BINDING_CAPACITY>>,
> = Mutex::new(RefCell::new(CallbackRouteTable::new()));
static CALLBACK_INTENTS: Mutex<
    CriticalSectionRawMutex,
    RefCell<CallbackActionQueue<SHELL_INTENT_QUEUE_CAPACITY>>,
> = Mutex::new(RefCell::new(CallbackActionQueue::new()));
static CALLBACK_ACTIONS: Mutex<
    CriticalSectionRawMutex,
    RefCell<Deque<IndexedAction, SHELL_INTENT_QUEUE_CAPACITY>>,
> = Mutex::new(RefCell::new(Deque::new()));
// Latest slider level for the one active value control. One bounded
// `Option` slot -- no new capacity constant, no extra collection (see
// `ValueSlot`). A fresh value for the pending (route, index) overwrites it;
// a different owner while one is pending raises the overflow diagnostic
// instead of queueing.
static CALLBACK_VALUE: Mutex<CriticalSectionRawMutex, RefCell<Option<ValueSlot>>> =
    Mutex::new(RefCell::new(None));
static CALLBACK_OVERFLOWED: AtomicBool = AtomicBool::new(false);
static AMBIENT_TAP_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn claim(bindings: IntentBindings) -> Result<CallbackLease, CallbackRouteError> {
    BINDINGS.lock(|routes| {
        routes
            .borrow_mut()
            .claim(bindings)
            .map(|route| CallbackLease { route })
    })
}

pub fn enable(lease: &CallbackLease) -> Result<(), CallbackRouteError> {
    BINDINGS.lock(|routes| routes.borrow_mut().enable(lease.route))
}

pub fn disable(lease: &CallbackLease) -> Result<(), CallbackRouteError> {
    BINDINGS.lock(|routes| routes.borrow_mut().disable(lease.route))
}

pub fn release(lease: &CallbackLease) -> Result<(), CallbackRouteError> {
    let released = BINDINGS.lock(|routes| routes.borrow_mut().release(lease.route));
    if released.is_ok() {
        // A coalesced latest value is a level reading from a live surface,
        // not a reliable edge like a click: once its route is gone the
        // reading is stale by definition, so drop it instead of delivering a
        // dead surface's level after teardown -- or after the slot is
        // re-claimed for a new generation, which must not deliver the old
        // level stale first.
        CALLBACK_VALUE.lock(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some_and(|pending| pending.route == lease.route) {
                slot.take();
            }
        });
    }
    released
}

pub fn take_intent() -> Option<OwnedShellIntent> {
    CALLBACK_INTENTS.lock(|intents| intents.borrow_mut().pop())
}

pub fn take_action() -> Option<IndexedAction> {
    CALLBACK_ACTIONS.lock(|actions| actions.borrow_mut().pop_front())
}

/// Consumes the pending coalesced slider level, if any. Rapid drags from
/// the pending (route, index) owner overwrite it in place rather than
/// queueing, separately from [`take_action`]'s reliable per-click ordering;
/// a value from any other owner while one is pending is dropped with the
/// overflow diagnostic set.
pub fn take_value_action() -> Option<IndexedValueAction> {
    CALLBACK_VALUE.lock(|slot| slot.borrow_mut().take().map(|pending| pending.action))
}

pub fn purge_instance(source: SurfaceInstanceToken) -> usize {
    let intents = CALLBACK_INTENTS.lock(|intents| intents.borrow_mut().purge_instance(source));
    let actions = CALLBACK_ACTIONS.lock(|actions| {
        let mut actions = actions.borrow_mut();
        let before = actions.len();
        actions.retain(|action| action.source != source);
        before - actions.len()
    });
    let values = CALLBACK_VALUE.lock(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some_and(|pending| pending.action.source == source) {
            slot.take();
            1
        } else {
            0
        }
    });
    intents + actions + values
}

#[cfg(feature = "ui-provider-fixture")]
pub fn purge_provider(owner: ProviderToken) -> usize {
    let intents = CALLBACK_INTENTS.lock(|intents| intents.borrow_mut().purge_provider(owner));
    let actions = CALLBACK_ACTIONS.lock(|actions| {
        let mut actions = actions.borrow_mut();
        let before = actions.len();
        actions.retain(|action| action.source.surface.owner != owner);
        before - actions.len()
    });
    let values = CALLBACK_VALUE.lock(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some_and(|pending| pending.action.source.surface.owner == owner) {
            slot.take();
            1
        } else {
            0
        }
    });
    intents + actions + values
}

#[cfg(feature = "ui-provider-fixture")]
pub fn queued_provider_action_count(owner: ProviderToken) -> usize {
    CALLBACK_INTENTS.lock(|intents| intents.borrow().provider_reference_count(owner))
        + CALLBACK_ACTIONS.lock(|actions| {
            actions
                .borrow()
                .iter()
                .filter(|action| action.source.surface.owner == owner)
                .count()
        })
        + CALLBACK_VALUE.lock(|slot| {
            usize::from(
                slot.borrow()
                    .is_some_and(|pending| pending.action.source.surface.owner == owner),
            )
        })
}

#[cfg(feature = "ui-provider-fixture")]
pub fn references_provider(owner: ProviderToken) -> bool {
    CALLBACK_INTENTS.lock(|intents| intents.borrow().references_provider(owner))
        || CALLBACK_ACTIONS.lock(|actions| {
            actions
                .borrow()
                .iter()
                .any(|action| action.source.surface.owner == owner)
        })
        || CALLBACK_VALUE.lock(|slot| {
            slot.borrow()
                .is_some_and(|pending| pending.action.source.surface.owner == owner)
        })
        || BINDINGS.lock(|routes| {
            routes
                .borrow()
                .any_value(|bindings| bindings.references_provider(owner))
        })
}

fn enqueue_action(encoded: u32, index: usize) {
    let Some(route) = CallbackRoute::from_encoded(encoded) else {
        return;
    };
    let action = BINDINGS.lock(|routes| {
        routes
            .borrow()
            .resolve(route)
            .and_then(|bindings| match bindings {
                IntentBindings::Action { source } => Some(IndexedAction { source, index }),
                IntentBindings::Screen { .. }
                | IntentBindings::Modal { .. }
                | IntentBindings::Refresh { .. } => None,
            })
    });
    let Some(action) = action else {
        return;
    };
    CALLBACK_ACTIONS.lock(|actions| {
        if actions.borrow_mut().push_back(action).is_err() {
            CALLBACK_OVERFLOWED.store(true, Ordering::Release);
        }
    });
}

fn enqueue_value(encoded: u32, index: usize, value: i32) {
    let Some(route) = CallbackRoute::from_encoded(encoded) else {
        return;
    };
    let source = BINDINGS.lock(|routes| {
        routes
            .borrow()
            .resolve(route)
            .and_then(|bindings| match bindings {
                IntentBindings::Action { source } => Some(source),
                IntentBindings::Screen { .. }
                | IntentBindings::Modal { .. }
                | IntentBindings::Refresh { .. } => None,
            })
    });
    let Some(source) = source else {
        return;
    };
    CALLBACK_VALUE.lock(|slot| {
        let mut slot = slot.borrow_mut();
        match slot.as_mut() {
            Some(pending) if pending.route == route && pending.action.index == index => {
                pending.action.value = value;
            }
            Some(_) => {
                // One active slider: a value from a different (route, index)
                // owner while one is pending is a capacity decision for a
                // future multi-control design, not a queue entry. Keep the
                // pending level untouched and raise the shared diagnostic.
                CALLBACK_OVERFLOWED.store(true, Ordering::Release);
            }
            None => {
                *slot = Some(ValueSlot {
                    route,
                    action: IndexedValueAction {
                        source,
                        index,
                        value,
                    },
                });
            }
        }
    });
}

pub fn take_overflowed() -> bool {
    CALLBACK_OVERFLOWED.swap(false, Ordering::AcqRel)
}

/// Consumes a pending Ambient Home background tap, if any. The Ambient Home
/// screen's own poll loop is responsible for discarding a stale flag left
/// over from a screen that is no longer active.
pub fn take_ambient_tap_requested() -> bool {
    AMBIENT_TAP_REQUESTED.swap(false, Ordering::AcqRel)
}

fn enqueue(encoded: u32, select: impl FnOnce(IntentBindings) -> Option<OwnedShellIntent>) {
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
