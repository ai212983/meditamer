use allocator_api2::{alloc::Allocator, boxed::Box, vec::Vec};
use core::sync::atomic::Ordering;

use super::{
    current_allocator_state, update_peak_used_bytes, AllocatorState, BufferAllocError,
    BufferPlacement, LargeByteBuffer, LargeByteBufferStorage, LARGE_ALLOC_EXTERNAL_OK,
    LARGE_ALLOC_FAIL, LARGE_ALLOC_INTERNAL_OK,
};

impl LargeByteBuffer {
    pub fn placement(&self) -> BufferPlacement {
        match self.storage {
            LargeByteBufferStorage::External(_) => BufferPlacement::Psram,
            LargeByteBufferStorage::Internal(_) => BufferPlacement::InternalRam,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        match &self.storage {
            LargeByteBufferStorage::External(bytes) => &bytes[..self.len],
            LargeByteBufferStorage::Internal(bytes) => &bytes[..self.len],
        }
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        match &mut self.storage {
            LargeByteBufferStorage::External(bytes) => &mut bytes[..self.len],
            LargeByteBufferStorage::Internal(bytes) => &mut bytes[..self.len],
        }
    }

    /// Converts a one-time firmware allocation into storage with device
    /// lifetime. The panel driver is created once and lives until reset, so
    /// retaining this allocation permanently is intentional.
    pub fn into_static_mut_slice(self) -> &'static mut [u8] {
        let len = self.len;
        match self.storage {
            LargeByteBufferStorage::External(bytes) => &mut Box::leak(bytes)[..len],
            LargeByteBufferStorage::Internal(bytes) => &mut Box::leak(bytes)[..len],
        }
    }
}

fn try_zeroed_box_in<A>(byte_len: usize, allocator: A) -> Result<Box<[u8], A>, BufferAllocError>
where
    A: Allocator,
{
    let mut bytes = Vec::new_in(allocator);
    bytes
        .try_reserve_exact(byte_len)
        .map_err(|_| BufferAllocError::OutOfMemory)?;
    bytes.resize(byte_len, 0);
    Ok(bytes.into_boxed_slice())
}

pub fn alloc_large_byte_buffer(byte_len: usize) -> Result<LargeByteBuffer, BufferAllocError> {
    if !matches!(current_allocator_state(), AllocatorState::Initialized) {
        return Err(BufferAllocError::AllocatorNotReady);
    }

    // Prefer PSRAM for large buffers to preserve internal-capability RAM for
    // Wi-Fi/radio allocations.
    let alloc_len = byte_len.max(1);
    let storage = if let Ok(bytes) = try_zeroed_box_in(alloc_len, esp_alloc::ExternalMemory) {
        LARGE_ALLOC_EXTERNAL_OK.fetch_add(1, Ordering::Relaxed);
        LargeByteBufferStorage::External(bytes)
    } else {
        let Ok(bytes) = try_zeroed_box_in(alloc_len, esp_alloc::InternalMemory) else {
            LARGE_ALLOC_FAIL.fetch_add(1, Ordering::Relaxed);
            return Err(BufferAllocError::OutOfMemory);
        };
        LARGE_ALLOC_INTERNAL_OK.fetch_add(1, Ordering::Relaxed);
        LargeByteBufferStorage::Internal(bytes)
    };
    let _ = update_peak_used_bytes(esp_alloc::HEAP.used());

    Ok(LargeByteBuffer {
        storage,
        len: byte_len,
    })
}
