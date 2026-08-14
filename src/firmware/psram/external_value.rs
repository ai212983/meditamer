use core::{
    alloc::{GlobalAlloc, Layout},
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use super::{
    current_allocator_state, update_peak_used_bytes, AllocatorState, BufferAllocError,
    LARGE_ALLOC_EXTERNAL_OK, LARGE_ALLOC_FAIL,
};

/// A value whose storage is guaranteed to reside in external PSRAM.
///
/// Callers must ensure it is not accessed by DMA or while flash/cache handling
/// makes PSRAM unavailable. The type provides placement, not that lifecycle
/// proof.
pub(crate) struct ExternalValue<T> {
    ptr: NonNull<T>,
}

const _: () = assert!(core::mem::size_of::<ExternalValue<u8>>() == core::mem::size_of::<usize>());

impl<T> ExternalValue<T> {
    pub(crate) fn try_new(value: T) -> Result<Self, BufferAllocError> {
        if !matches!(current_allocator_state(), AllocatorState::Initialized) {
            return Err(BufferAllocError::AllocatorNotReady);
        }
        let layout = Layout::new::<T>();
        if layout.size() == 0 {
            return Err(BufferAllocError::OutOfMemory);
        }
        let raw = unsafe {
            esp_alloc::HEAP.alloc_caps(esp_alloc::MemoryCapability::External.into(), layout)
        };
        let Some(ptr) = NonNull::new(raw.cast::<T>()) else {
            LARGE_ALLOC_FAIL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            return Err(BufferAllocError::OutOfMemory);
        };
        unsafe { ptr.as_ptr().write(value) };
        LARGE_ALLOC_EXTERNAL_OK.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let _ = update_peak_used_bytes(esp_alloc::HEAP.used());
        Ok(Self { ptr })
    }

    pub(crate) fn into_inner(self) -> T {
        let value = unsafe { self.ptr.as_ptr().read() };
        let ptr = self.ptr;
        core::mem::forget(self);
        unsafe {
            GlobalAlloc::dealloc(
                &esp_alloc::HEAP,
                ptr.as_ptr().cast::<u8>(),
                Layout::new::<T>(),
            );
        }
        value
    }
}

impl<T> Deref for ExternalValue<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.as_ref() }
    }
}

impl<T> DerefMut for ExternalValue<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.ptr.as_mut() }
    }
}

impl<T> Drop for ExternalValue<T> {
    fn drop(&mut self) {
        unsafe {
            self.ptr.as_ptr().drop_in_place();
            GlobalAlloc::dealloc(
                &esp_alloc::HEAP,
                self.ptr.as_ptr().cast::<u8>(),
                Layout::new::<T>(),
            );
        }
    }
}

unsafe impl<T: Send> Send for ExternalValue<T> {}
