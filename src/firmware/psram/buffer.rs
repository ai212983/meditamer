use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
    slice,
    sync::atomic::Ordering,
};

use super::{
    current_allocator_state, update_peak_used_bytes, AllocatorState, BufferAllocError,
    BufferPlacement, LargeByteBuffer, LARGE_ALLOC_EXTERNAL_OK, LARGE_ALLOC_FAIL,
    LARGE_ALLOC_INTERNAL_OK,
};

impl LargeByteBuffer {
    pub(crate) fn placement(&self) -> BufferPlacement {
        self.placement
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    /// Converts a one-time firmware allocation into storage with device
    /// lifetime. The panel driver is created once and lives until reset, so
    /// retaining this allocation permanently is intentional.
    pub(crate) fn into_static_mut_slice(self) -> &'static mut [u8] {
        let buffer = core::mem::ManuallyDrop::new(self);
        unsafe { slice::from_raw_parts_mut(buffer.ptr.as_ptr(), buffer.len) }
    }
}

impl Drop for LargeByteBuffer {
    fn drop(&mut self) {
        unsafe {
            GlobalAlloc::dealloc(&esp_alloc::HEAP, self.ptr.as_ptr(), self.layout);
        }
    }
}
pub(crate) fn alloc_large_byte_buffer(
    byte_len: usize,
) -> Result<LargeByteBuffer, BufferAllocError> {
    if !matches!(current_allocator_state(), AllocatorState::Initialized) {
        return Err(BufferAllocError::AllocatorNotReady);
    }

    let alloc_len = byte_len.max(1);
    let layout =
        Layout::from_size_align(alloc_len, 1).map_err(|_| BufferAllocError::OutOfMemory)?;
    // Prefer PSRAM for large buffers to preserve internal-capability RAM for
    // Wi-Fi/radio allocations.
    let external_ptr =
        unsafe { esp_alloc::HEAP.alloc_caps(esp_alloc::MemoryCapability::External.into(), layout) };
    let (ptr, placement) = if let Some(ptr) = NonNull::new(external_ptr) {
        LARGE_ALLOC_EXTERNAL_OK.fetch_add(1, Ordering::Relaxed);
        (ptr, BufferPlacement::Psram)
    } else {
        let internal_ptr = unsafe {
            esp_alloc::HEAP.alloc_caps(esp_alloc::MemoryCapability::Internal.into(), layout)
        };
        if let Some(ptr) = NonNull::new(internal_ptr) {
            LARGE_ALLOC_INTERNAL_OK.fetch_add(1, Ordering::Relaxed);
            (ptr, BufferPlacement::InternalRam)
        } else {
            LARGE_ALLOC_FAIL.fetch_add(1, Ordering::Relaxed);
            return Err(BufferAllocError::OutOfMemory);
        }
    };
    unsafe {
        core::ptr::write_bytes(ptr.as_ptr(), 0, alloc_len);
    }
    let _ = update_peak_used_bytes(esp_alloc::HEAP.used());

    Ok(LargeByteBuffer {
        placement,
        ptr,
        len: byte_len,
        layout,
    })
}
