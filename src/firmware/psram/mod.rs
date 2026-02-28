#![allow(dead_code)]

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
#[cfg(feature = "psram-alloc")]
use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
    slice,
};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AllocatorState {
    Disabled,
    NotInitialized,
    Initialized,
    InitFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AllocatorStatus {
    pub(crate) feature_enabled: bool,
    pub(crate) state: AllocatorState,
    pub(crate) total_bytes: usize,
    pub(crate) free_bytes: usize,
    pub(crate) peak_used_bytes: usize,
}

static ALLOCATOR_STATE: AtomicU8 = AtomicU8::new(initial_allocator_state());
static PEAK_USED_BYTES: AtomicUsize = AtomicUsize::new(0);
static LAST_LOGGED_PEAK_USED_BYTES: AtomicUsize = AtomicUsize::new(0);
const INTERNAL_HEAP_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BufferPlacement {
    InternalRam,
    Psram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BufferAllocError {
    AllocatorDisabled,
    AllocatorNotReady,
    OutOfMemory,
}

pub(crate) struct LargeByteBuffer {
    placement: BufferPlacement,
    #[cfg(feature = "psram-alloc")]
    ptr: NonNull<u8>,
    #[cfg(feature = "psram-alloc")]
    len: usize,
    #[cfg(feature = "psram-alloc")]
    layout: Layout,
}

#[cfg(feature = "psram-alloc")]
unsafe impl Send for LargeByteBuffer {}
#[cfg(feature = "psram-alloc")]
unsafe impl Sync for LargeByteBuffer {}

const fn initial_allocator_state() -> u8 {
    if cfg!(feature = "psram-alloc") {
        AllocatorState::NotInitialized as u8
    } else {
        AllocatorState::Disabled as u8
    }
}

fn allocator_state_from_u8(raw: u8) -> AllocatorState {
    match raw {
        0 => AllocatorState::Disabled,
        1 => AllocatorState::NotInitialized,
        2 => AllocatorState::Initialized,
        3 => AllocatorState::InitFailed,
        _ => AllocatorState::InitFailed,
    }
}

fn allocator_state_raw(state: AllocatorState) -> u8 {
    state as u8
}

fn current_allocator_state() -> AllocatorState {
    allocator_state_from_u8(ALLOCATOR_STATE.load(Ordering::Relaxed))
}

fn update_allocator_state(state: AllocatorState) {
    ALLOCATOR_STATE.store(allocator_state_raw(state), Ordering::Relaxed);
}

fn used_bytes(total_bytes: usize, free_bytes: usize) -> usize {
    total_bytes.saturating_sub(free_bytes)
}

fn update_peak_used_bytes(used: usize) -> usize {
    let mut peak = PEAK_USED_BYTES.load(Ordering::Relaxed);
    while used > peak {
        match PEAK_USED_BYTES.compare_exchange_weak(
            peak,
            used,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return used,
            Err(observed) => peak = observed,
        }
    }
    peak
}

fn maybe_log_new_peak(tag: &str, peak_used_bytes: usize, total_bytes: usize, free_bytes: usize) {
    let mut last_logged = LAST_LOGGED_PEAK_USED_BYTES.load(Ordering::Relaxed);
    while peak_used_bytes > last_logged {
        match LAST_LOGGED_PEAK_USED_BYTES.compare_exchange_weak(
            last_logged,
            peak_used_bytes,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                esp_println::println!(
                    "psram: high_water tag={} peak_used_bytes={} total_bytes={} free_bytes={}",
                    tag,
                    peak_used_bytes,
                    total_bytes,
                    free_bytes
                );
                break;
            }
            Err(observed) => last_logged = observed,
        }
    }
}

impl LargeByteBuffer {
    pub(crate) fn placement(&self) -> BufferPlacement {
        self.placement
    }

    pub(crate) fn len(&self) -> usize {
        #[cfg(feature = "psram-alloc")]
        {
            self.len
        }
        #[cfg(not(feature = "psram-alloc"))]
        {
            0
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[cfg(feature = "psram-alloc")]
    pub(crate) fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    #[cfg(feature = "psram-alloc")]
    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

#[cfg(feature = "psram-alloc")]
impl Drop for LargeByteBuffer {
    fn drop(&mut self) {
        unsafe {
            GlobalAlloc::dealloc(&esp_alloc::HEAP, self.ptr.as_ptr(), self.layout);
        }
    }
}

#[cfg(feature = "psram-alloc")]
pub(crate) fn init_allocator(psram: &esp_hal::peripherals::PSRAM<'_>) -> AllocatorStatus {
    if matches!(current_allocator_state(), AllocatorState::Initialized) {
        return allocator_status();
    }

    // Keep an internal-capability heap region for subsystems (Wi-Fi) that
    // cannot allocate from external PSRAM.
    esp_alloc::heap_allocator!(size: INTERNAL_HEAP_BYTES);

    let (_start, size) = esp_hal::psram::psram_raw_parts(psram);
    if size == 0 {
        update_allocator_state(AllocatorState::InitFailed);
        return allocator_status();
    }

    esp_alloc::psram_allocator!(psram, esp_hal::psram);
    PEAK_USED_BYTES.store(0, Ordering::Relaxed);
    LAST_LOGGED_PEAK_USED_BYTES.store(0, Ordering::Relaxed);
    update_allocator_state(AllocatorState::Initialized);
    allocator_status()
}

#[cfg(not(feature = "psram-alloc"))]
pub(crate) fn init_allocator() -> AllocatorStatus {
    allocator_status()
}

#[cfg(feature = "psram-alloc")]
pub(crate) fn alloc_large_byte_buffer(
    byte_len: usize,
) -> Result<LargeByteBuffer, BufferAllocError> {
    if !matches!(current_allocator_state(), AllocatorState::Initialized) {
        return Err(BufferAllocError::AllocatorNotReady);
    }

    let alloc_len = byte_len.max(1);
    let layout =
        Layout::from_size_align(alloc_len, 1).map_err(|_| BufferAllocError::OutOfMemory)?;
    let ptr = unsafe { GlobalAlloc::alloc(&esp_alloc::HEAP, layout) };
    let Some(ptr) = NonNull::new(ptr) else {
        return Err(BufferAllocError::OutOfMemory);
    };
    unsafe {
        core::ptr::write_bytes(ptr.as_ptr(), 0, alloc_len);
    }
    let _ = update_peak_used_bytes(esp_alloc::HEAP.used());

    Ok(LargeByteBuffer {
        placement: BufferPlacement::Psram,
        ptr,
        len: byte_len,
        layout,
    })
}

#[cfg(not(feature = "psram-alloc"))]
pub(crate) fn alloc_large_byte_buffer(
    _byte_len: usize,
) -> Result<LargeByteBuffer, BufferAllocError> {
    Err(BufferAllocError::AllocatorDisabled)
}

pub(crate) fn allocator_status() -> AllocatorStatus {
    #[cfg(feature = "psram-alloc")]
    let (total_bytes, free_bytes) = {
        let stats = esp_alloc::HEAP.stats();
        (stats.size, esp_alloc::HEAP.free())
    };
    #[cfg(not(feature = "psram-alloc"))]
    let (total_bytes, free_bytes) = (0, 0);
    let peak_used_bytes = update_peak_used_bytes(used_bytes(total_bytes, free_bytes));

    if cfg!(feature = "psram-alloc") {
        AllocatorStatus {
            feature_enabled: true,
            state: current_allocator_state(),
            total_bytes,
            free_bytes,
            peak_used_bytes,
        }
    } else {
        AllocatorStatus {
            feature_enabled: false,
            state: AllocatorState::Disabled,
            total_bytes,
            free_bytes,
            peak_used_bytes,
        }
    }
}

pub(crate) fn log_allocator_status() {
    let status = allocator_status();
    esp_println::println!(
        "psram: feature_enabled={} state={:?} total_bytes={} free_bytes={} peak_used_bytes={}",
        status.feature_enabled,
        status.state,
        status.total_bytes,
        status.free_bytes,
        status.peak_used_bytes
    );
}

pub(crate) fn log_allocator_high_water(tag: &str) {
    let status = allocator_status();
    if !status.feature_enabled || !matches!(status.state, AllocatorState::Initialized) {
        return;
    }

    maybe_log_new_peak(
        tag,
        status.peak_used_bytes,
        status.total_bytes,
        status.free_bytes,
    );
}
