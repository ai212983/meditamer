use core::{
    alloc::{GlobalAlloc, Layout},
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use super::{current_allocator_state, update_peak_used_bytes, AllocatorState, BufferAllocError};

/// A value whose storage is guaranteed to reside in internal RAM.
///
/// This is used for futures that may execute while the flash cache is disabled.
/// Pinning remains the caller's responsibility; the allocation itself never moves.
pub(crate) struct InternalValue<T> {
    ptr: NonNull<T>,
}

const _: () = assert!(core::mem::size_of::<InternalValue<u8>>() == core::mem::size_of::<usize>());

impl<T> InternalValue<T> {
    pub(crate) fn try_new(value: T) -> Result<Self, BufferAllocError> {
        if !matches!(current_allocator_state(), AllocatorState::Initialized) {
            return Err(BufferAllocError::AllocatorNotReady);
        }
        let layout = Layout::new::<T>();
        if layout.size() == 0 {
            return Err(BufferAllocError::OutOfMemory);
        }
        let raw = unsafe {
            esp_alloc::HEAP.alloc_caps(esp_alloc::MemoryCapability::Internal.into(), layout)
        };
        let Some(ptr) = NonNull::new(raw.cast::<T>()) else {
            return Err(BufferAllocError::OutOfMemory);
        };
        unsafe { ptr.as_ptr().write(value) };
        let _ = update_peak_used_bytes(esp_alloc::HEAP.used());
        Ok(Self { ptr })
    }

    #[inline(never)]
    pub(crate) fn try_new_bounded<const MAX: usize, Make>(
        make: Make,
    ) -> Result<Self, BufferAllocError>
    where
        Make: FnOnce() -> T,
    {
        const { assert!(core::mem::size_of::<T>() <= MAX) }
        Self::try_new(make())
    }
}

impl<T> Deref for InternalValue<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.as_ref() }
    }
}

impl<T> DerefMut for InternalValue<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.ptr.as_mut() }
    }
}

impl<T> Drop for InternalValue<T> {
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

unsafe impl<T: Send> Send for InternalValue<T> {}
