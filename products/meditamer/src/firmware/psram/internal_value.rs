use allocator_api2::boxed::Box;
use core::{
    cell::Cell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    pin::Pin,
};

use super::{
    current_allocator_state, initialized_box, update_peak_used_bytes, AllocatorState,
    BufferAllocError,
};

/// A value whose storage is guaranteed to reside in internal RAM.
///
/// This is used for futures that may execute while the flash cache is disabled.
/// The allocation itself never moves, so [`Self::pin_mut`] can safely pin the
/// contained value for a borrow tied to this owner.
pub struct InternalValue<T> {
    inner: Pin<Box<T, esp_alloc::InternalMemory>>,
    _not_sync: PhantomData<Cell<()>>,
}

const _: () = assert!(core::mem::size_of::<InternalValue<u8>>() == core::mem::size_of::<usize>());

impl<T> InternalValue<T> {
    fn try_new_with<Make>(make: Make) -> Result<Self, BufferAllocError>
    where
        Make: FnOnce() -> T,
    {
        if !matches!(current_allocator_state(), AllocatorState::Initialized) {
            return Err(BufferAllocError::AllocatorNotReady);
        }
        if core::mem::size_of::<T>() == 0 {
            return Err(BufferAllocError::OutOfMemory);
        }
        let slot = Box::<T, _>::try_new_uninit_in(esp_alloc::InternalMemory)
            .map_err(|_| BufferAllocError::OutOfMemory)?;
        let inner = Box::into_pin(initialized_box::write(slot, make));
        let _ = update_peak_used_bytes(esp_alloc::HEAP.used());
        Ok(Self {
            inner,
            _not_sync: PhantomData,
        })
    }

    #[inline(never)]
    pub(crate) fn try_new_bounded<const MAX: usize, Make>(
        make: Make,
    ) -> Result<Self, BufferAllocError>
    where
        Make: FnOnce() -> T,
    {
        const { assert!(core::mem::size_of::<T>() <= MAX) }
        Self::try_new_with(make)
    }

    /// Pins the contained value at its allocation address until it is dropped.
    #[inline]
    pub(crate) fn pin_mut(&mut self) -> Pin<&mut T> {
        self.inner.as_mut()
    }
}

impl<T> Deref for InternalValue<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().get_ref()
    }
}

impl<T: Unpin> DerefMut for InternalValue<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut().get_mut()
    }
}
