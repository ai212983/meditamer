#![allow(dead_code)]
#![deny(unsafe_code)]

//! PSRAM-aware global allocator.
//!
//! The allocator state, its counters, and the buffer types live here; [`init`]
//! brings the allocator up, [`buffer`] does large placement-aware allocations,
//! and [`status`] reports usage and high-water marks.

mod buffer;
mod bundles;
mod external_value;
#[allow(unsafe_code)]
mod init;
#[allow(unsafe_code)]
mod initialized_box;
mod internal_value;
#[allow(unsafe_code)]
mod provenance;
mod status;

pub use buffer::alloc_large_byte_buffer;
pub use bundles::{acquire_runtime_bundle, RuntimeBundleAcquireError, MAX_RUNTIME_BUNDLE_BYTES};
pub use bundles::{prepare_external_bundle, ExternalBundleError};
#[cfg(any(
    feature = "sd-runner-allocation-fixture",
    feature = "ui-initialization-fixture"
))]
pub(crate) use external_value::reject_next_allocation as reject_next_external_allocation;
pub use external_value::ExternalValue;
pub use init::init_allocator;
pub use internal_value::InternalValue;
#[cfg(feature = "asset-upload-http")]
pub use status::probe_internal_block_above_reserve;
pub use status::{
    allocator_memory_snapshot, allocator_status, log_allocator_high_water, log_allocator_status,
};

use allocator_api2::boxed::Box;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocatorState {
    Disabled,
    NotInitialized,
    Initialized,
    InitFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocatorStatus {
    pub feature_enabled: bool,
    pub state: AllocatorState,
    pub total_bytes: usize,
    pub free_bytes: usize,
    pub peak_used_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocatorMemorySnapshot {
    pub feature_enabled: bool,
    pub state: AllocatorState,
    pub total_bytes: usize,
    pub used_bytes: usize,
    pub free_bytes: usize,
    pub peak_used_bytes: usize,
    pub free_internal_bytes: usize,
    pub free_external_bytes: usize,
    pub min_free_bytes: usize,
    pub min_free_internal_bytes: usize,
    // The seven `min_internal_alloc_*` correlation-diagnostic fields below
    // stay crate-private (product/target axis completion plan, Phase 5):
    // `net_host.rs`'s port adapter reads the fourteen fields above only,
    // matching `netstack::host::AllocatorMemorySnapshot`'s narrower mirror
    // (Phase 4, E-0007) -- nothing outside this crate reads these.
    pub(crate) min_internal_alloc_charge_bytes: usize,
    pub(crate) min_internal_alloc_internal_required: bool,
    pub(crate) min_internal_alloc_charge_overflow: bool,
    pub(crate) min_internal_alloc_post_free_bytes: usize,
    pub(crate) min_internal_alloc_correlation_stable: bool,
    pub(crate) min_internal_alloc_wifi_rx_matched: bool,
    pub(crate) min_internal_alloc_released: bool,
    pub min_free_external_bytes: usize,
    pub large_alloc_external_ok: usize,
    pub large_alloc_internal_ok: usize,
    pub large_alloc_fail: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternalBlockProbe {
    pub free_before_bytes: usize,
    pub block_bytes: usize,
    pub reserve_bytes: usize,
    pub free_after_bytes: usize,
    pub stable: bool,
}

static ALLOCATOR_STATE: AtomicU8 = AtomicU8::new(initial_allocator_state());
static PEAK_USED_BYTES: AtomicUsize = AtomicUsize::new(0);
static LAST_LOGGED_PEAK_USED_BYTES: AtomicUsize = AtomicUsize::new(0);
static MIN_FREE_BYTES: AtomicUsize = AtomicUsize::new(usize::MAX);
static MIN_FREE_EXTERNAL_BYTES: AtomicUsize = AtomicUsize::new(usize::MAX);
static LARGE_ALLOC_EXTERNAL_OK: AtomicUsize = AtomicUsize::new(0);
static LARGE_ALLOC_INTERNAL_OK: AtomicUsize = AtomicUsize::new(0);
static LARGE_ALLOC_FAIL: AtomicUsize = AtomicUsize::new(0);
/// The always-present internal-capability heap, in `dram2_seg`
/// (`.dram2_uninit`). BLE-linked builds add the controller's permanently
/// released 15,448-byte Classic Bluetooth range as a second internal region.
///
/// It deliberately takes nothing from `dram_seg`: `.stack` is whatever is left
/// of `dram_seg` after `.data`/`.bss`, so heap placed there comes straight out
/// of the CPU0 stack. `dram2_seg` cannot back the stack at all, which makes it
/// free capacity by comparison. The Classic-only range is already excluded
/// from `dram_seg` by the BTDM reservation, so registering it adds real internal
/// capacity without reducing `.stack`. `dram2_seg` also holds the 45000 byte
/// `FRAMEBUFFER_BW`, so growing this constant past the remainder of its 113840
/// bytes fails at link time.
///
/// The 64 KiB baseline left 3,304 bytes unused in `dram2_seg`. Exact Phase 1S
/// evidence correlated two live vendor RX packets with a 3,400-byte heap
/// excursion and a 13,508-byte low-water, so 3,200 bytes of that tail belongs
/// to the sole internal-capability heap while retaining a small link-time tail.
///
/// Do not add another internal region in the reclaimed PRO CPU ROM stack: that
/// was measured at an 11/40 boot panic rate. The Classic region is second and
/// leaves esp-alloc's third and final region slot for PSRAM. It still requires
/// the documented 40-boot/device radio qualification. See
/// docs/references/memory/meditamer-inkplate/rom-stack.md.
const INTERNAL_HEAP_DRAM2_BYTES: usize = 64 * 1024 + 3_200;
#[cfg(feature = "classic-bt-memory-reclaim")]
const CLASSIC_BT_HEAP_BYTES: usize = 15_448;
#[cfg(feature = "classic-bt-memory-reclaim")]
const MAX_INTERNAL_HEAP_BYTES: usize = INTERNAL_HEAP_DRAM2_BYTES + CLASSIC_BT_HEAP_BYTES;
#[cfg(not(feature = "classic-bt-memory-reclaim"))]
const MAX_INTERNAL_HEAP_BYTES: usize = INTERNAL_HEAP_DRAM2_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferPlacement {
    InternalRam,
    Psram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferAllocError {
    AllocatorNotReady,
    OutOfMemory,
}

enum LargeByteBufferStorage {
    External(Box<[u8], esp_alloc::ExternalMemory>),
    Internal(Box<[u8], esp_alloc::InternalMemory>),
}

pub struct LargeByteBuffer {
    storage: LargeByteBufferStorage,
    len: usize,
}

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
                if !crate::firmware::update::transport_quiet() {
                    console::println!(
                        "psram: high_water tag={} peak_used_bytes={} total_bytes={} free_bytes={}",
                        tag,
                        peak_used_bytes,
                        total_bytes,
                        free_bytes
                    );
                }
                break;
            }
            Err(observed) => last_logged = observed,
        }
    }
}
