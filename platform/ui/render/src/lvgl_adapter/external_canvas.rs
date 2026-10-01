//! Boot-lifetime externally allocated L8 canvas storage.
//!
//! [`StaticL8CanvasBuffer`](super::StaticL8CanvasBuffer) requires spelling a
//! huge `[u8; N]` value at construction, which fits neither the embedded
//! stack nor internal static RAM at 360KB scale (the Meditamer SD-clock
//! ambient canvas lives in PSRAM). [`ExternalL8CanvasBuffer`] instead adopts
//! one caller-allocated boot-lifetime byte buffer (`&'static mut [u8]`,
//! typically a single leaked PSRAM allocation) and upholds the same
//! token-gated writer/attach contract. The wrapper itself is a second small
//! `&'static` allocation; no safe API ever exposes the retained bytes.

#![allow(unsafe_code)]

use core::cell::UnsafeCell;

use lightvgl_sys as lv;

use super::retained::RetainedResourceError;
use super::{AccessFault, UiAccessToken};

/// Small stable descriptor for boot-lifetime L8 pixel storage owned outside
/// this adapter (e.g. one leaked PSRAM byte buffer behind the SD-clock
/// ambient canvas).
///
/// LVGL retains the byte address rather than copying pixels, so both the
/// bytes and this descriptor must stay put for the life of the runtime: leak
/// each exactly once at boot and never reclaim either. Requiring
/// `&'static self` for attachment and mutation keeps that address stable;
/// requiring the current [`UiAccessToken`] serializes Rust writes with every
/// LVGL draw operation on the one owning UI task.
///
/// Mutating storage never invalidates the widget; repaint through
/// [`with_writer`](Self::with_writer), then call the existing checked
/// [`Widget::invalidate`](super::Widget::invalidate), exactly as for
/// [`StaticL8CanvasBuffer`](super::StaticL8CanvasBuffer).
pub struct ExternalL8CanvasBuffer {
    ptr: UnsafeCell<*mut u8>,
    len: usize,
}

impl ExternalL8CanvasBuffer {
    /// Adopts exclusive boot-lifetime ownership of `bytes`.
    ///
    /// The caller guarantees `bytes` is uniquely owned, lives forever (a
    /// boot-leaked allocation), and is never accessed again except through
    /// this buffer's token-gated writers. Construction fills the storage
    /// white (`0xFF`) in place through the exclusive borrow, so no
    /// raw-pointer initialization is needed after sharing.
    ///
    /// # Errors
    ///
    /// Returns [`RetainedResourceError::InvalidExternalCanvasBuffer`] for
    /// empty storage or storage whose address is not four-byte aligned, which
    /// LVGL's L8 canvas path requires just as it does for the static buffer.
    pub fn new(bytes: &'static mut [u8]) -> Result<Self, RetainedResourceError> {
        if bytes.is_empty() {
            return Err(RetainedResourceError::InvalidExternalCanvasBuffer);
        }
        if !(bytes.as_ptr() as usize).is_multiple_of(4) {
            return Err(RetainedResourceError::InvalidExternalCanvasBuffer);
        }
        bytes.fill(0xFF);
        let len = bytes.len();
        let ptr = bytes.as_mut_ptr();
        // SAFETY: the caller transferred exclusive boot-lifetime ownership;
        // the borrow ends here and the referent is never named through it
        // again. Every later access is a token-serialized, bounds-checked raw
        // write on the single UI owner that also serializes LVGL's retained
        // reads, and the address/len pair is immutable from this point on.
        Ok(Self {
            ptr: UnsafeCell::new(ptr),
            len,
        })
    }

    /// Retained byte capacity.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Always false: construction rejects empty storage.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Provides bounded pixel-writing operations on the current UI runtime
    /// epoch without ever exposing a Rust reference to the retained bytes.
    pub fn with_writer<R>(
        &'static self,
        token: &UiAccessToken,
        update: impl FnOnce(&mut ExternalL8CanvasWriter<'_>) -> R,
    ) -> Result<R, AccessFault> {
        token.check_current()?;
        Ok(update(&mut ExternalL8CanvasWriter {
            storage: self,
            token,
        }))
    }

    pub(super) fn as_mut_ptr(&'static self) -> *mut u8 {
        // SAFETY: the pointer is immutable after construction; copying it out
        // hands the caller no Rust reference, only provenance LVGL retains.
        unsafe { *self.ptr.get() }
    }

    /// Hands the retained address to one `lv_canvas`. The widget facade owns
    /// the kind/dimension/capacity checks and passes its epoch proof; this
    /// keeps the single `lv_canvas_set_buffer` call site for external storage
    /// quarantined here, next to the buffer it retains.
    pub(super) fn attach_to_canvas(
        &'static self,
        _: &super::raw::UiCapability<'_>,
        object_addr: usize,
        width: i32,
        height: i32,
    ) {
        // SAFETY: the facade checked Canvas kind, positive geometry, and
        // capacity fit, and validated the current UI epoch (the capability
        // proof). `object_addr` is the exposed provenance of a live
        // registered `lv_obj_t`, mirroring the parked-widget retry path, and
        // the retained bytes are boot-lifetime `'static`, four-byte aligned,
        // and mutated only through token-serialized writers.
        unsafe {
            lv::lv_canvas_set_buffer(
                core::ptr::with_exposed_provenance_mut(object_addr),
                self.as_mut_ptr().cast(),
                width,
                height,
                lv::lv_color_format_t_LV_COLOR_FORMAT_L8,
            )
        };
    }
}

// SAFETY: the raw pointer (and its `UnsafeCell` wrapper) is the sole reason
// automatic `Sync` is unavailable. Safe mutation requires the current
// non-Send/non-Sync UI token and exposes only non-callback raw writes, so the
// single UI owner is the only execution context that can access the bytes
// while LVGL retains their address — the same justification as
// `StaticL8CanvasBuffer`'s `Sync`.
unsafe impl Sync for ExternalL8CanvasBuffer {}

/// Closure-scoped write access to one externally owned L8 canvas.
///
/// This deliberately does not implement `DerefMut`, `AsMut`, or expose a
/// slice. A caller may re-enter a token-checked LVGL operation between writer
/// calls, but no Rust reference into the retained bytes is live at that point.
pub struct ExternalL8CanvasWriter<'a> {
    storage: &'a ExternalL8CanvasBuffer,
    token: &'a UiAccessToken,
}

impl ExternalL8CanvasWriter<'_> {
    /// Fills every retained byte. Returns false without mutation when the UI
    /// epoch already moved on.
    pub fn fill(&mut self, value: u8) -> bool {
        if self.token.check_current().is_err() {
            return false;
        }
        // SAFETY: the epoch recheck serializes this with LVGL's retained
        // reads on the one UI owner; the `len` bytes are valid by
        // construction. No reference is created and no user callback runs
        // until this write has completed.
        unsafe { core::ptr::write_bytes(*self.storage.ptr.get(), value, self.storage.len) };
        true
    }

    /// Writes one byte. Returns false without mutation for an out-of-range
    /// offset or an expired UI epoch.
    pub fn write(&mut self, offset: usize, value: u8) -> bool {
        if self.token.check_current().is_err() || offset >= self.storage.len {
            return false;
        }
        // SAFETY: the bounds check keeps the write inside the retained
        // storage. As with `fill`, no reference exists and no user callback
        // runs until this write has completed.
        unsafe { (*self.storage.ptr.get()).add(offset).write(value) };
        true
    }

    /// Copies `src` into storage at `offset`. Returns false without mutation
    /// when the range overflows storage or the UI epoch already moved on.
    pub fn copy_from(&mut self, offset: usize, src: &[u8]) -> bool {
        if self.token.check_current().is_err() {
            return false;
        }
        match offset.checked_add(src.len()) {
            Some(end) if end <= self.storage.len => {}
            _ => return false,
        }
        // SAFETY: the checked range bounds the destination; `src` is a
        // caller-held slice that cannot alias storage (no safe reference to
        // storage exists). The epoch recheck serializes this with LVGL.
        unsafe {
            core::ptr::copy_nonoverlapping(
                src.as_ptr(),
                (*self.storage.ptr.get()).add(offset),
                src.len(),
            )
        };
        true
    }

    /// Expands row-major MSB-first ink bits into L8 pixels: a set bit paints
    /// ink (`0x00`), a clear bit paints paper (`0xFF`). Writes at most
    /// `pixel_count` pixels, clamped to storage capacity and to the bits
    /// available, and returns the pixel count written.
    pub fn write_ink_bits(&mut self, bits: &[u8], pixel_count: usize) -> usize {
        if self.token.check_current().is_err() {
            return 0;
        }
        let count = pixel_count
            .min(self.storage.len)
            .min(bits.len().saturating_mul(8));
        // SAFETY: `count` fits both storage and `bits` by construction, and
        // the epoch recheck serializes these raw writes with LVGL's retained
        // reads. No reference into either side escapes the loop.
        unsafe {
            let dst = *self.storage.ptr.get();
            let src = bits.as_ptr();
            for index in 0..count {
                let byte = *src.add(index / 8);
                let ink = (byte >> (7 - (index % 8))) & 1 != 0;
                dst.add(index).write(if ink { 0x00 } else { 0xFF });
            }
        }
        count
    }
}
