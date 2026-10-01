//! Checked LVGL access boundary (ADR-0020).
//!
//! [`RuntimeSession`] owns one runtime epoch; [`Widget`] owns one registered
//! object generation. Raw pointers and C calls remain private to [`raw`],
//! while callback ABI decoding lives in the dedicated callback modules.

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
use heapless::Vec;

mod external_canvas;
mod orphaned;
mod packed;
mod prepared_l8;
mod raw;
mod retained;
mod runtime;
mod runtime_io;
mod widget;

#[cfg(test)]
use core::ffi::CStr;
pub use external_canvas::{ExternalL8CanvasBuffer, ExternalL8CanvasWriter};
#[cfg(test)]
use lightvgl_sys as lv;
pub use orphaned::{
    park_orphaned_widget, park_orphaned_widget_or_panic, retry_parked_widgets,
    ORPHANED_WIDGET_CAPACITY,
};
pub use packed::{
    i1_canvas_buffer_bytes, indexed_canvas_buffer_bytes, I1CanvasWriter, IndexedCanvasWriter,
    MonochromePixel, PaletteColor, StaticI1CanvasBuffer, StaticIndexedCanvasBuffer,
};
pub use prepared_l8::{PreparedL8DrawStats, PreparedL8DrawUnit};
pub use retained::{
    L8CanvasWriter, LinePoint, LinePointsWriter, RetainedResourceError, StaticFontRef,
    StaticL8CanvasBuffer, StaticL8DrawBuffer, StaticLinePoints,
};
pub use runtime::{
    is_initialized, LvglMemorySnapshot, RuntimeSession, RuntimeSessionError, UnmanagedScreen,
    UnmanagedScreenDeleteFailure,
};
pub use runtime_io::{Display, DisplayCreateError, FlushFrame, FlushFrameError};
#[cfg(feature = "lvgl-gestures")]
pub use runtime_io::{
    GestureDirection, GestureEvent, PointerContact, PointerInput, PointerInputAccess,
    PointerInputCallbacks, PointerInputCreateError, PointerSample, PointerState,
};
#[cfg(test)]
pub(crate) use widget::registry_check;
pub(crate) use widget::registry_contains_addr;
pub use widget::{
    black, white, AccessFault, Align, Font, StyleState, TextAlign, Widget, WidgetCallbackError,
    WidgetCreateError, WidgetDeleteFailure, WidgetIdentity, WidgetKind, RADIUS_CIRCLE,
};

/// Upper bound on widgets simultaneously registered through this adapter.
/// Every Meditamer presentation root registers its own widgets here.
/// Sized against today's actual compiled catalogue (three launchable
/// entries, not `CompiledCatalogue`'s eight-entry structural ceiling), for
/// the worst pairing this adapter must hold at once: the widest screen twice
/// over -- `execute_transition` keeps origin and candidate both live through
/// commit -- plus a modal and the sticky refresh control on top. Concretely:
/// two catalogue-shaped screens (~16 each: root, title, 3 rows of 3, plus the
/// 5-widget carousel) at 32, a Confirm or Settings modal (~7) at 39, and the
/// refresh control (~2) at 41. 64 keeps roughly 50% headroom above that for
/// growth in live composition size, without the >=72-widget cost of actually
/// ceiling. Raising the widget registry from 24 to 64 costs 320 static bytes;
/// see the DRAM budget for the measured per-board delta.
pub const WIDGET_REGISTRY_CAPACITY: usize = 64;

type Registry<const N: usize> = Mutex<CriticalSectionRawMutex, RefCell<Vec<(usize, u32), N>>>;

static WIDGET_RUNTIME_ID: AtomicU32 = AtomicU32::new(0);
static WIDGET_GENERATION: AtomicU32 = AtomicU32::new(0);
static UNMANAGED_SCREEN_CAPTURED: AtomicBool = AtomicBool::new(false);
static WIDGET_REGISTRY: Registry<WIDGET_REGISTRY_CAPACITY> = Mutex::new(RefCell::new(Vec::new()));

/// Draws the next value from a global, monotonically increasing counter —
/// never reused, so a stale handle can never coincidentally match a later
/// object at a reused address. Single-owning-task access (the same contract
/// [`UiAccessToken`] establishes for everything else here) makes a plain
/// load/store race-free; this mirrors `ShellModel::issue_instance`'s
/// `checked_add`, one level below its `ShellNavigationError::
/// InstanceGenerationExhausted`.
fn next_generation(counter: &AtomicU32) -> Option<u32> {
    let next = counter.load(Ordering::Acquire).checked_add(1)?;
    counter.store(next, Ordering::Release);
    Some(next)
}

#[cfg(test)]
fn registry_upsert<const N: usize>(registry: &Registry<N>, addr: usize, generation: u32) -> bool {
    registry.lock(|entries| {
        let mut entries = entries.borrow_mut();
        if let Some(entry) = entries.iter_mut().find(|(a, _)| *a == addr) {
            entry.1 = generation;
            true
        } else {
            entries.push((addr, generation)).is_ok()
        }
    })
}

/// Reserves a registry slot before LVGL creates an object. The zero address
/// is never a valid LVGL object address, so it is a private reservation
/// marker; the UI task is the sole caller and commits it immediately after
/// creation.
fn registry_reserve<const N: usize>(registry: &Registry<N>, generation: u32) -> bool {
    registry.lock(|entries| entries.borrow_mut().push((0, generation)).is_ok())
}

fn registry_commit<const N: usize>(registry: &Registry<N>, generation: u32, addr: usize) -> bool {
    registry.lock(|entries| {
        entries
            .borrow_mut()
            .iter_mut()
            .find(|(entry_addr, entry_generation)| {
                *entry_addr == 0 && *entry_generation == generation
            })
            .map(|entry| {
                entry.0 = addr;
            })
            .is_some()
    })
}

fn registry_release_reservation<const N: usize>(registry: &Registry<N>, generation: u32) {
    registry.lock(|entries| {
        entries
            .borrow_mut()
            .retain(|(addr, entry_generation)| *addr != 0 || *entry_generation != generation)
    });
}

fn registry_remove<const N: usize>(registry: &Registry<N>, addr: usize) {
    registry.lock(|entries| entries.borrow_mut().retain(|(a, _)| *a != addr));
}

/// Proof that the holder is the UI task driving the current LVGL runtime.
/// Every adapter operation requires one. Tokens can only be obtained from the
/// session that owns the corresponding runtime epoch.
///
/// `Clone` makes every additional holder explicit; the raw-pointer marker
/// keeps the UI-owner capability from crossing task boundaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiAccessToken {
    runtime_id: u32,
    _not_send_or_sync: core::marker::PhantomData<*const ()>,
}

impl UiAccessToken {
    fn check_current(&self) -> Result<(), AccessFault> {
        if self.runtime_id != WIDGET_RUNTIME_ID.load(Ordering::Acquire) {
            return Err(AccessFault::WrongRuntime);
        }
        Ok(())
    }

    fn capability(&self) -> Result<raw::UiCapability<'_>, AccessFault> {
        self.check_current()?;
        Ok(raw::UiCapability::after_validation(self))
    }

    /// Issues a token for a new LVGL runtime epoch, clearing every checked
    /// handle registry and the orphaned-widget retry queue,
    /// so a stale `(addr, generation)` entry from a prior epoch cannot linger
    /// and eventually exhaust a registry's bounded capacity across repeated
    /// LVGL reinit cycles. Individual stale handles are also rejected by
    /// [`Widget::check`]'s epoch comparison against the now-current
    /// [`WIDGET_RUNTIME_ID`] without this -- this only reclaims the registry
    /// space they otherwise held onto forever. A widget parked through
    /// [`park_orphaned_widget`] under a prior epoch has no LVGL object left
    /// to retry against once that epoch's runtime is gone.
    ///
    fn issue() -> Self {
        let runtime_id = WIDGET_RUNTIME_ID
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        WIDGET_REGISTRY.lock(|entries| entries.borrow_mut().clear());
        UNMANAGED_SCREEN_CAPTURED.store(false, Ordering::Release);
        orphaned::clear();
        Self {
            runtime_id,
            _not_send_or_sync: core::marker::PhantomData,
        }
    }
}

fn invalidate_runtime(_runtime_id: u32) {
    WIDGET_RUNTIME_ID.fetch_add(1, Ordering::AcqRel);
    WIDGET_REGISTRY.lock(|entries| entries.borrow_mut().clear());
    UNMANAGED_SCREEN_CAPTURED.store(false, Ordering::Release);
    orphaned::clear();
}

#[cfg(test)]
#[path = "lvgl_adapter/tests.rs"]
mod tests;
