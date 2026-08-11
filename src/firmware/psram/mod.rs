#![allow(dead_code)]

//! PSRAM-aware global allocator.
//!
//! The allocator state, its counters, and the buffer types live here; [`init`]
//! brings the allocator up, [`buffer`] does large placement-aware allocations,
//! and [`status`] reports usage and high-water marks.

mod buffer;
mod init;
mod status;

pub(crate) use buffer::alloc_large_byte_buffer;
pub(crate) use init::init_allocator;
pub(crate) use status::{
    allocator_memory_snapshot, allocator_status, log_allocator_high_water, log_allocator_status,
};

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use core::{alloc::Layout, ptr::NonNull};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AllocatorMemorySnapshot {
    pub(crate) feature_enabled: bool,
    pub(crate) state: AllocatorState,
    pub(crate) total_bytes: usize,
    pub(crate) used_bytes: usize,
    pub(crate) free_bytes: usize,
    pub(crate) peak_used_bytes: usize,
    pub(crate) free_internal_bytes: usize,
    pub(crate) free_external_bytes: usize,
    pub(crate) min_free_bytes: usize,
    pub(crate) min_free_internal_bytes: usize,
    pub(crate) min_free_external_bytes: usize,
    pub(crate) large_alloc_external_ok: usize,
    pub(crate) large_alloc_internal_ok: usize,
    pub(crate) large_alloc_fail: usize,
}

static ALLOCATOR_STATE: AtomicU8 = AtomicU8::new(initial_allocator_state());
static PEAK_USED_BYTES: AtomicUsize = AtomicUsize::new(0);
static LAST_LOGGED_PEAK_USED_BYTES: AtomicUsize = AtomicUsize::new(0);
static MIN_FREE_BYTES: AtomicUsize = AtomicUsize::new(usize::MAX);
static MIN_FREE_INTERNAL_BYTES: AtomicUsize = AtomicUsize::new(usize::MAX);
static MIN_FREE_EXTERNAL_BYTES: AtomicUsize = AtomicUsize::new(usize::MAX);
static LARGE_ALLOC_EXTERNAL_OK: AtomicUsize = AtomicUsize::new(0);
static LARGE_ALLOC_INTERNAL_OK: AtomicUsize = AtomicUsize::new(0);
static LARGE_ALLOC_FAIL: AtomicUsize = AtomicUsize::new(0);
/// The whole internal-capability heap, in `dram2_seg` (`.dram2_uninit`).
///
/// It deliberately takes nothing from `dram_seg`: `.stack` is whatever is left
/// of `dram_seg` after `.data`/`.bss`, so heap placed there comes straight out
/// of the CPU0 stack. `dram2_seg` cannot back the stack at all, which makes it
/// free capacity by comparison. It also holds the 45000 byte `FRAMEBUFFER_BW`,
/// so growing this past the remainder of its 113840 bytes fails at link time.
///
/// Do not add a second internal region in the reclaimed PRO CPU ROM stack: that
/// was measured at an 11/40 boot panic rate. See
/// docs/reference/dram-budget-rom-stack.md.
const INTERNAL_HEAP_DRAM2_BYTES: usize = 58 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BufferPlacement {
    InternalRam,
    Psram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BufferAllocError {
    AllocatorNotReady,
    OutOfMemory,
}

pub(crate) struct LargeByteBuffer {
    placement: BufferPlacement,
    ptr: NonNull<u8>,
    len: usize,
    layout: Layout,
}

unsafe impl Send for LargeByteBuffer {}
unsafe impl Sync for LargeByteBuffer {}

const fn initial_allocator_state() -> u8 {
    AllocatorState::NotInitialized as u8
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

fn update_min_observed(atom: &AtomicUsize, value: usize) -> usize {
    let mut current = atom.load(Ordering::Relaxed);
    while value < current {
        match atom.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return value,
            Err(observed) => current = observed,
        }
    }
    current
}

fn min_or_zero(value: usize) -> usize {
    if value == usize::MAX {
        0
    } else {
        value
    }
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
