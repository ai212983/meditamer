use allocator_api2::boxed::Box;
use core::{
    cell::Cell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    pin::Pin,
};

use super::{
    current_allocator_state, initialized_box, update_peak_used_bytes, AllocatorState,
    BufferAllocError, LARGE_ALLOC_EXTERNAL_OK, LARGE_ALLOC_FAIL,
};

/// A value whose storage is guaranteed to reside in external PSRAM.
///
/// Callers must ensure it is not accessed by DMA or while flash/cache handling
/// makes PSRAM unavailable. The type provides placement, not that lifecycle
/// proof. The allocation itself never moves, so [`Self::pin_mut`] can safely
/// pin the contained value for a borrow tied to this owner.
pub struct ExternalValue<T> {
    inner: Pin<Box<T, esp_alloc::ExternalMemory>>,
    _not_sync: PhantomData<Cell<()>>,
}

const _: () = assert!(core::mem::size_of::<ExternalValue<u8>>() == core::mem::size_of::<usize>());

// Diagnostic images only. Startup fixtures arm and consume this in
// one synchronous call, before any runner polling or other task can interleave.
#[cfg(any(
    feature = "sd-runner-allocation-fixture",
    feature = "ui-initialization-fixture"
))]
static REJECT_NEXT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[cfg(any(
    feature = "sd-runner-allocation-fixture",
    feature = "ui-initialization-fixture"
))]
pub(crate) fn reject_next_allocation() {
    assert!(!REJECT_NEXT.swap(true, core::sync::atomic::Ordering::Relaxed));
}

impl<T> ExternalValue<T> {
    pub fn try_new_with<Make>(make: Make) -> Result<Self, BufferAllocError>
    where
        Make: FnOnce() -> T,
    {
        if !matches!(current_allocator_state(), AllocatorState::Initialized) {
            return Err(BufferAllocError::AllocatorNotReady);
        }
        if core::mem::size_of::<T>() == 0 {
            return Err(BufferAllocError::OutOfMemory);
        }
        #[cfg(any(
            feature = "sd-runner-allocation-fixture",
            feature = "ui-initialization-fixture"
        ))]
        let reject = REJECT_NEXT.swap(false, core::sync::atomic::Ordering::Relaxed);
        #[cfg(not(any(
            feature = "sd-runner-allocation-fixture",
            feature = "ui-initialization-fixture"
        )))]
        let reject = false;
        if reject {
            LARGE_ALLOC_FAIL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            return Err(BufferAllocError::OutOfMemory);
        }
        let slot = Box::<T, _>::try_new_uninit_in(esp_alloc::ExternalMemory).map_err(|_| {
            LARGE_ALLOC_FAIL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            BufferAllocError::OutOfMemory
        })?;
        let inner = Box::into_pin(initialized_box::write(slot, make));
        LARGE_ALLOC_EXTERNAL_OK.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let _ = update_peak_used_bytes(esp_alloc::HEAP.used());
        Ok(Self {
            inner,
            _not_sync: PhantomData,
        })
    }

    pub fn into_inner(self) -> T
    where
        T: Unpin,
    {
        Box::into_inner(Pin::into_inner(self.inner))
    }

    /// Retain a boot-lifetime resource after a C API takes its stable address.
    /// The caller must arrange at most one allocation per retained resource.
    pub fn leak(self) -> &'static mut T
    where
        T: Unpin + 'static,
    {
        Box::leak(Pin::into_inner(self.inner))
    }

    /// Pins the contained value at its allocation address until it is dropped.
    #[inline]
    pub fn pin_mut(&mut self) -> Pin<&mut T> {
        self.inner.as_mut()
    }
}

impl<T> Deref for ExternalValue<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().get_ref()
    }
}

impl<T: Unpin> DerefMut for ExternalValue<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut().get_mut()
    }
}
