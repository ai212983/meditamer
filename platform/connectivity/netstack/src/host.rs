//! The product surface the network owner supervises, and netstack-owned
//! listener/admission state the product's HTTP service reads inward.
//!
//! The product supplies the work to supervise, state to poll, and its
//! networking-enabled policy bit. The platform depends only on these shapes,
//! preserving the product-to-platform dependency direction.
//!
//! The split into two mechanisms is deliberate, and follows the call graph:
//!
//! * The two **async** entry points (`serve`, `abort_service_work`) are a
//!   generic [`NetHost`] bound. They cannot be `dyn` -- `async` methods are
//!   not dyn-compatible without boxing and there is no allocator on this path
//!   -- but they are needed in only two functions, so the generic threads a
//!   short way and stops. It has to stay generic all the way up to the
//!   `#[embassy_executor::task]` that starts it, since a task cannot be
//!   generic.
//!
//! * The **sync** reads and the one policy callback are plain `fn` pointers
//!   installed once at startup. They are consumed deep inside
//!   `resource_snapshot`, which has many call sites across the supervision
//!   ladder; making all of those generic to reach a handful of counter reads
//!   would be far more invasive than the coupling it removes.
//!
//! Listener enable state, its change sequence, and radio-handoff admission
//! are netstack-owned operational state, not a product callback: the product
//! reads them inward through this module's plain functions.

use core::{
    cell::Cell,
    future::Future,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use critical_section::Mutex;
use embassy_net::Stack;

/// Async product work supervised across a network epoch.
pub trait NetHost {
    /// The product's serving work for one epoch, created fresh from that
    /// epoch's stack and joined alongside the connection and runner futures.
    /// Returning means the services ended, which the supervisor treats as a
    /// fault.
    fn serve(&self, stack: Stack<'_>) -> impl Future<Output = ()>;

    /// Drain in-flight service work before radio handoff. Doubles as the
    /// SD-task FIFO fence: a correlated completion proves earlier work
    /// drained before radio ownership is released. `true` when the abort was
    /// acknowledged.
    fn abort_service_work(&self) -> impl Future<Output = bool>;
}

/// A plain-data mirror of a product allocator's contiguous-block probe.
/// Owned here rather than reused from a product's allocator type, so this
/// port's only cross-crate dependency stays zero: the shape is pure
/// arithmetic data, not a real type-sharing need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternalBlockProbe {
    pub free_before_bytes: usize,
    pub block_bytes: usize,
    pub reserve_bytes: usize,
    pub free_after_bytes: usize,
    pub stable: bool,
}

/// A plain-data mirror of a product allocator's overall status, used by the
/// Wi-Fi driver's own memory diagnostics (low-mem recovery logging). Same
/// zero-cross-crate-type-dependency reasoning as [`InternalBlockProbe`].
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocatorState {
    Disabled,
    NotInitialized,
    Initialized,
    InitFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocatorMemorySnapshot {
    pub feature_enabled: bool,
    pub state: AllocatorState,
    pub total_bytes: usize,
    pub used_bytes: usize,
    pub free_bytes: usize,
    pub peak_used_bytes: usize,
    pub free_internal_bytes: usize,
    pub free_external_bytes: usize,
    pub min_free_bytes: usize,
    pub min_free_internal_bytes: usize,
    pub min_free_external_bytes: usize,
    pub large_alloc_external_ok: usize,
    pub large_alloc_internal_ok: usize,
    pub large_alloc_fail: usize,
}

/// Sync product state the supervisor polls while deciding whether the radio
/// can change hands, plus the one product policy callback and the two
/// allocator diagnostics a target adapter converts from its own allocator's
/// types.
pub struct ProductState {
    pub active_http_connections: fn() -> u16,
    pub active_sd_roundtrips: fn() -> u16,
    pub upload_session_active: fn() -> bool,
    /// True while a firmware update needs the transport left alone.
    pub transport_quiet: fn() -> bool,
    /// True while the product currently enables networking (Meditamer maps
    /// this to its upload-enabled app state).
    pub upload_enabled: fn() -> bool,
    /// The allocator's current overall status, used by radio-handoff memory
    /// floor checks (`.free_internal_bytes` only) and by the Wi-Fi driver's
    /// low-memory diagnostic logging (the full snapshot).
    pub allocator_memory_snapshot: fn() -> AllocatorMemorySnapshot,
    /// Probe the largest contiguous internal-capability block above the
    /// given reserve.
    pub probe_internal_block_above_reserve: fn(usize) -> InternalBlockProbe,
}

static PRODUCT: Mutex<Cell<Option<&'static ProductState>>> = Mutex::new(Cell::new(None));

/// Install the product's state accessors. Called once during startup, before
/// the network owner task is spawned.
pub fn install(state: &'static ProductState) {
    critical_section::with(|cs| PRODUCT.borrow(cs).set(Some(state)));
}

fn product() -> Option<&'static ProductState> {
    critical_section::with(|cs| PRODUCT.borrow(cs).get())
}

// Before `install`, every reader reports "no product work in flight" (or, for
// `upload_enabled`, "disabled"). That is only observable if the owner task is
// spawned without a product installed, which startup does not do; the
// supervisor would otherwise read uninitialised state as busy and refuse to
// ever hand the radio over.
pub fn active_http_connections() -> u16 {
    product().map_or(0, |state| (state.active_http_connections)())
}

pub fn active_sd_roundtrips() -> u16 {
    product().map_or(0, |state| (state.active_sd_roundtrips)())
}

pub fn upload_session_active() -> bool {
    product().is_some_and(|state| (state.upload_session_active)())
}

pub fn transport_quiet() -> bool {
    product().is_some_and(|state| (state.transport_quiet)())
}

pub fn upload_enabled() -> bool {
    product().is_some_and(|state| (state.upload_enabled)())
}

pub fn allocator_memory_snapshot() -> AllocatorMemorySnapshot {
    product().map_or(
        AllocatorMemorySnapshot {
            feature_enabled: false,
            state: AllocatorState::NotInitialized,
            total_bytes: 0,
            used_bytes: 0,
            free_bytes: 0,
            peak_used_bytes: 0,
            free_internal_bytes: 0,
            free_external_bytes: 0,
            min_free_bytes: 0,
            min_free_internal_bytes: 0,
            min_free_external_bytes: 0,
            large_alloc_external_ok: 0,
            large_alloc_internal_ok: 0,
            large_alloc_fail: 0,
        },
        |state| (state.allocator_memory_snapshot)(),
    )
}

pub fn probe_internal_block_above_reserve(reserve_bytes: usize) -> InternalBlockProbe {
    product().map_or(
        InternalBlockProbe {
            free_before_bytes: 0,
            block_bytes: 0,
            reserve_bytes,
            free_after_bytes: 0,
            stable: true,
        },
        |state| (state.probe_internal_block_above_reserve)(reserve_bytes),
    )
}

// Netstack-owned listener and radio-handoff admission state. The HTTP
// service reads it inward through the plain functions below rather than
// through a product callback -- it is network policy and operational state,
// not product state the network owner polls.
static LISTENER_ENABLED: AtomicBool = AtomicBool::new(true);
static LISTENER_SET_SEQ: AtomicU32 = AtomicU32::new(0);
static RADIO_HANDOFF_ADMISSION_OPEN: AtomicBool = AtomicBool::new(true);

pub fn listener_enabled() -> bool {
    LISTENER_ENABLED.load(Ordering::Relaxed)
}

pub fn listener_set_seq() -> u32 {
    LISTENER_SET_SEQ.load(Ordering::Relaxed)
}

/// Set by the serial `NET LISTENER ON|OFF` command.
pub fn set_listener_enabled(enabled: bool) {
    LISTENER_ENABLED.store(enabled, Ordering::Relaxed);
    LISTENER_SET_SEQ.fetch_add(1, Ordering::Relaxed);
}

pub fn radio_handoff_admission_open() -> bool {
    RADIO_HANDOFF_ADMISSION_OPEN.load(Ordering::Acquire)
}

pub(crate) fn set_radio_handoff_admission_open(open: bool) {
    RADIO_HANDOFF_ADMISSION_OPEN.store(open, Ordering::Release);
}
