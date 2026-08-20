//! The product surface the network owner supervises.
//!
//! ADR-0015 Tier 2. `net/runtime.rs` used to call `firmware::storage::upload`
//! and `firmware::update` directly, which is the wrong direction once `net`
//! becomes `platform/netstack`: the platform would depend on the product. This
//! module inverts it — the product supplies the work to supervise and the state
//! to poll, and the supervisor knows only these shapes.
//!
//! The split into two mechanisms is deliberate, and follows the call graph:
//!
//! * The two **async** entry points (`serve`, `abort_upload`) are a generic
//!   [`NetHost`] bound. They cannot be `dyn` — `async` methods are not
//!   dyn-compatible without boxing and there is no allocator on this path — but
//!   they are needed in only two functions, so the generic threads a short way
//!   and stops. It has to stay generic all the way up to the
//!   `#[embassy_executor::task]` that starts it, since a task cannot be generic.
//!
//! * The four **sync** reads are plain `fn` pointers installed once at startup.
//!   They are consumed deep inside `resource_snapshot`, which has thirteen call
//!   sites across the supervision ladder; making all of those generic to reach
//!   four counter reads would be far more invasive than the coupling it removes.

use core::{
    future::Future,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use embassy_net::Stack;

/// Async product work supervised across a network epoch.
pub(crate) trait NetHost {
    /// The product's serving work for one epoch, created fresh from that
    /// epoch's stack and joined alongside the connection and runner futures.
    /// Returning means the services ended, which the supervisor treats as a
    /// fault.
    fn serve(&self, stack: Stack<'_>) -> impl Future<Output = ()>;

    /// Force in-flight upload work to finish. Doubles as the SD-task FIFO
    /// fence: a correlated completion proves earlier work drained before radio
    /// ownership is released. `true` when the abort was acknowledged.
    fn abort_upload(&self) -> impl Future<Output = bool>;
}

/// Sync product state the supervisor polls while deciding whether the radio can
/// change hands.
pub(crate) struct ProductState {
    pub(crate) active_http_connections: fn() -> u16,
    pub(crate) active_sd_roundtrips: fn() -> u16,
    pub(crate) upload_session_active: fn() -> bool,
    /// True while a firmware update needs the transport left alone.
    pub(crate) transport_quiet: fn() -> bool,
}

static PRODUCT: AtomicPtr<ProductState> = AtomicPtr::new(ptr::null_mut());

/// Install the product's state accessors. Called once during startup, before
/// the network owner task is spawned.
pub(crate) fn install(state: &'static ProductState) {
    PRODUCT.store(
        state as *const ProductState as *mut ProductState,
        Ordering::Release,
    );
}

fn product() -> Option<&'static ProductState> {
    // Published by `install` from a `&'static` before the owner task exists.
    unsafe { PRODUCT.load(Ordering::Acquire).as_ref() }
}

// Before `install`, every reader reports "no product work in flight". That is
// only observable if the owner task is spawned without one, which startup does
// not do; the supervisor would otherwise read uninitialised state as busy and
// refuse to ever hand the radio over.
pub(crate) fn active_http_connections() -> u16 {
    product().map_or(0, |state| (state.active_http_connections)())
}

pub(crate) fn active_sd_roundtrips() -> u16 {
    product().map_or(0, |state| (state.active_sd_roundtrips)())
}

pub(crate) fn upload_session_active() -> bool {
    product().is_some_and(|state| (state.upload_session_active)())
}

pub(crate) fn transport_quiet() -> bool {
    product().is_some_and(|state| (state.transport_quiet)())
}
