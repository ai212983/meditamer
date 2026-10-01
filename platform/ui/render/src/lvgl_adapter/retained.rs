//! Stable storage retained by LVGL after an adapter call returns.

#![allow(unsafe_code)]

use core::cell::UnsafeCell;

use lightvgl_sys as lv;

use super::{AccessFault, UiAccessToken};

/// Why a retained canvas or line resource was rejected before LVGL retained
/// its pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetainedResourceError {
    Access(AccessFault),
    /// Canvas width and height must both be positive and their product must
    /// fit in `usize` on the target.
    InvalidCanvasDimensions,
    /// The static L8 storage has fewer than `width * height` bytes.
    CanvasBufferTooSmall,
    /// Externally allocated L8 storage was empty or not four-byte aligned;
    /// LVGL retains the address, so it must be usable as L8 canvas backing.
    InvalidExternalCanvasBuffer,
    /// LVGL rejected the configured packed-canvas layout.
    InvalidCanvasLayout,
    /// A line needs at least one retained point.
    EmptyLinePoints,
    /// LVGL's line API represents its retained point count as `u32`.
    TooManyLinePoints,
}

/// Reusable four-byte-aligned static storage retained by an LVGL display.
///
/// A [`super::RuntimeSession`] serializes access but permits multiple displays;
/// use a distinct buffer per display to avoid rendering-state interference.
/// No safe API exposes the bytes.
#[repr(align(4))]
pub struct StaticL8DrawBuffer<const WORDS: usize> {
    words: UnsafeCell<[u32; WORDS]>,
}

impl<const WORDS: usize> StaticL8DrawBuffer<WORDS> {
    pub const fn new() -> Self {
        Self {
            words: UnsafeCell::new([0; WORDS]),
        }
    }

    pub const fn capacity_bytes(&self) -> usize {
        WORDS * core::mem::size_of::<u32>()
    }

    pub(super) fn as_mut_ptr(&'static self) -> *mut core::ffi::c_void {
        self.words.get().cast()
    }
}

impl<const WORDS: usize> Default for StaticL8DrawBuffer<WORDS> {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: the sole RuntimeSession serializes LVGL retention, and safe code can
// neither read nor mutate the storage directly.
unsafe impl<const WORDS: usize> Sync for StaticL8DrawBuffer<WORDS> {}

impl From<AccessFault> for RetainedResourceError {
    fn from(fault: AccessFault) -> Self {
        Self::Access(fault)
    }
}

/// Static, four-byte-aligned L8 pixel storage whose interior mutability stays
/// behind the UI-owner capability.
///
/// LVGL retains this buffer's address rather than copying its pixels. Requiring
/// `&'static self` for both attachment and mutation makes that address stable;
/// requiring the current [`UiAccessToken`] serializes Rust writes with every
/// LVGL draw operation on the one owning UI task.
#[repr(align(4))]
pub struct StaticL8CanvasBuffer<const N: usize> {
    bytes: UnsafeCell<[u8; N]>,
}

impl<const N: usize> StaticL8CanvasBuffer<N> {
    pub const fn new(bytes: [u8; N]) -> Self {
        Self {
            bytes: UnsafeCell::new(bytes),
        }
    }

    pub const fn len(&self) -> usize {
        N
    }

    pub const fn is_empty(&self) -> bool {
        N == 0
    }

    /// Provides bounded pixel-writing operations on the current UI runtime
    /// epoch without ever exposing a Rust reference to the retained bytes.
    pub fn with_writer<R>(
        &'static self,
        token: &UiAccessToken,
        update: impl FnOnce(&mut L8CanvasWriter<'_, N>) -> R,
    ) -> Result<R, AccessFault> {
        token.check_current()?;
        Ok(update(&mut L8CanvasWriter {
            storage: self,
            token,
        }))
    }

    pub(super) fn as_mut_ptr(&'static self) -> *mut u8 {
        self.bytes.get().cast::<u8>()
    }
}

// SAFETY: `UnsafeCell` is the sole reason automatic `Sync` is unavailable.
// Safe mutation requires the current non-Send/non-Sync UI token and exposes
// only non-callback raw writes, so the single UI owner is the only execution
// context that can access the bytes while LVGL retains their address.
unsafe impl<const N: usize> Sync for StaticL8CanvasBuffer<N> {}

/// Closure-scoped write access to one retained L8 canvas.
///
/// This deliberately does not implement `DerefMut`, `AsMut`, or expose a
/// slice. A caller may re-enter a token-checked LVGL operation between writer
/// calls, but no Rust reference into the retained bytes is live at that point.
pub struct L8CanvasWriter<'a, const N: usize> {
    storage: &'a StaticL8CanvasBuffer<N>,
    token: &'a UiAccessToken,
}

impl<const N: usize> L8CanvasWriter<'_, N> {
    pub fn fill(&mut self, value: u8) -> bool {
        if self.token.check_current().is_err() {
            return false;
        }
        // SAFETY: this operation rechecks the UI epoch and the writer
        // cannot leave its owning thread or closure. This non-callback operation creates no
        // Rust reference and completes before user code can re-enter LVGL.
        unsafe { core::ptr::write_bytes(self.storage.bytes.get().cast::<u8>(), value, N) };
        true
    }

    /// Returns false without mutation for an invalid index or expired UI epoch.
    pub fn set(&mut self, index: usize, value: u8) -> bool {
        if self.token.check_current().is_err() || index >= N {
            return false;
        }
        // SAFETY: the bounds check keeps the write within the static array.
        // As with `fill`, no reference exists and no user callback runs until
        // this write has completed.
        unsafe {
            self.storage
                .bytes
                .get()
                .cast::<u8>()
                .add(index)
                .write(value)
        };
        true
    }
}

/// Render-owned representation of LVGL's precise line point.
///
/// Both fields deliberately use LVGL's configured scalar alias. With the
/// repository's `LV_USE_FLOAT=1` configurations this is `f32`; if that setting
/// drifts, [`LinePoint::new`] stops compiling rather than silently passing
/// float bits to an integer-configured LVGL.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinePoint {
    x: lv::lv_value_precise_t,
    y: lv::lv_value_precise_t,
}

impl LinePoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub const fn x(self) -> f32 {
        self.x
    }

    pub const fn y(self) -> f32 {
        self.y
    }
}

/// Static point storage for an LVGL line, with mutation serialized by the
/// current UI-owner capability just like [`StaticL8CanvasBuffer`].
#[repr(transparent)]
pub struct StaticLinePoints<const N: usize> {
    points: UnsafeCell<[LinePoint; N]>,
}

impl<const N: usize> StaticLinePoints<N> {
    pub const fn new(points: [LinePoint; N]) -> Self {
        Self {
            points: UnsafeCell::new(points),
        }
    }

    pub const fn len(&self) -> usize {
        N
    }

    pub const fn is_empty(&self) -> bool {
        N == 0
    }

    pub fn with_writer<R>(
        &'static self,
        token: &UiAccessToken,
        update: impl FnOnce(&mut LinePointsWriter<'_, N>) -> R,
    ) -> Result<R, AccessFault> {
        token.check_current()?;
        Ok(update(&mut LinePointsWriter {
            storage: self,
            token,
        }))
    }

    pub(super) fn as_ptr(&'static self) -> *const LinePoint {
        self.points.get().cast_const().cast::<LinePoint>()
    }
}

// SAFETY: all safe mutation is gated by the current non-Send/non-Sync UI token
// and exposes only non-callback raw writes; LVGL reads the retained address
// only on that same serialized owner.
unsafe impl<const N: usize> Sync for StaticLinePoints<N> {}

/// Closure-scoped point writes without a reference into LVGL-retained storage.
pub struct LinePointsWriter<'a, const N: usize> {
    storage: &'a StaticLinePoints<N>,
    token: &'a UiAccessToken,
}

impl<const N: usize> LinePointsWriter<'_, N> {
    /// Returns false without mutation for an invalid index or expired UI epoch.
    pub fn set(&mut self, index: usize, point: LinePoint) -> bool {
        if self.token.check_current().is_err() || index >= N {
            return false;
        }
        // SAFETY: the index is in bounds, `LinePoint` has no drop glue, and
        // this non-callback raw write ends before control returns to user code.
        unsafe {
            self.storage
                .points
                .get()
                .cast::<LinePoint>()
                .add(index)
                .write(point)
        };
        true
    }
}

/// A shared reference to a validated, compiled-in font table.
#[derive(Clone, Copy)]
pub struct StaticFontRef(&'static lv::lv_font_t);

impl StaticFontRef {
    /// Wraps a font emitted by a generator that validates the complete LVGL
    /// table and gives all retained data static storage.
    ///
    /// # Safety
    ///
    /// `font` and every pointer reachable through its descriptors must refer
    /// to initialized, mutually consistent data that remains valid forever.
    /// Its callback pointers must implement LVGL's font ABI for those exact
    /// descriptors. A merely `'static` `lv_font_t` does not satisfy this.
    pub const unsafe fn from_generated(font: &'static lv::lv_font_t) -> Self {
        Self(font)
    }

    pub(super) const fn get(self) -> &'static lv::lv_font_t {
        self.0
    }
}

impl core::fmt::Debug for StaticFontRef {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_tuple("StaticFontRef")
            .field(&(core::ptr::from_ref(self.0) as usize))
            .finish()
    }
}

impl PartialEq for StaticFontRef {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.0, other.0)
    }
}

impl Eq for StaticFontRef {}
