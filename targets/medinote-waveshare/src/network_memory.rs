//! Target-owned radio heap and byte-only external memory.
//!
//! PSRAM is not registered with the global allocator: controller objects,
//! atomics, task state and DMA workspaces retain internal memory placement.
use core::sync::atomic::{AtomicBool, Ordering};
use netstack::host::{AllocatorMemorySnapshot, AllocatorState, InternalBlockProbe};

pub const INTERNAL_HEAP_BYTES: usize = 65_536;
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Install the one internal allocator region after bootloader startup.
pub fn init_heap() {
    assert!(
        !INITIALIZED.swap(true, Ordering::AcqRel),
        "radio heap initialized twice"
    );
    #[esp_hal::ram(reclaimed)]
    static mut RADIO_HEAP: core::mem::MaybeUninit<[u8; INTERNAL_HEAP_BYTES]> =
        // stack-risk-reviewed: NOLOAD static in HAL reclaimed RAM, never a CPU-stack array.
        core::mem::MaybeUninit::uninit();
    // SAFETY: this NOLOAD region belongs exclusively to this allocator after
    // the ESP-IDF bootloader returns. The once guard prevents aliasing regions.
    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            core::ptr::addr_of_mut!(RADIO_HEAP).cast::<u8>(),
            INTERNAL_HEAP_BYTES,
            esp_alloc::MemoryCapability::Internal.into(),
        ));
    }
}

/// Map board octal PSRAM and lend only the requested byte buffer to its owner.
/// The peripheral is consumed once and no allocator can alias this memory.
pub fn init_bytes(
    peripheral: esp_hal::peripherals::PSRAM<'static>,
    bytes: usize,
) -> &'static mut [u8] {
    let psram = esp_hal::psram::Psram::new(
        peripheral,
        esp_hal::psram::PsramConfig {
            mode: esp_hal::psram::PsramMode::OctalSpi,
            ..Default::default()
        },
    );
    let (start, available) = psram.raw_parts();
    assert!(
        !start.is_null() && available >= bytes,
        "network PSRAM unavailable"
    );
    console::println!(
        "NETWORK_MEMORY psram_bytes={} reserved_bytes={} allocator_external=false",
        available,
        bytes
    );
    // Psram has no teardown; keep the consumed peripheral alive until this
    // function returns. Deep sleep reconstructs it.
    let _psram = psram;
    // SAFETY: the consumed peripheral exclusively owns this mapped region;
    // byte slices contain neither atomics nor DMA descriptors, and no other
    // owner/allocator receives any of its range.
    let buffer = unsafe { core::slice::from_raw_parts_mut(start, bytes) };
    buffer.fill(0);
    buffer
}

pub fn snapshot() -> AllocatorMemorySnapshot {
    let stats = esp_alloc::HEAP.stats();
    let free = esp_alloc::HEAP.free();
    AllocatorMemorySnapshot {
        feature_enabled: true,
        state: AllocatorState::Initialized,
        total_bytes: INTERNAL_HEAP_BYTES,
        used_bytes: stats.current_usage,
        free_bytes: free,
        peak_used_bytes: stats.max_usage,
        free_internal_bytes: free,
        free_external_bytes: 0,
        min_free_bytes: INTERNAL_HEAP_BYTES.saturating_sub(stats.max_usage),
        min_free_internal_bytes: INTERNAL_HEAP_BYTES.saturating_sub(stats.max_usage),
        min_free_external_bytes: 0,
        large_alloc_external_ok: 0,
        large_alloc_internal_ok: 0,
        large_alloc_fail: 0,
    }
}

/// Measure an allocatable lower bound while retaining the owner's reserve.
pub fn probe(reserve_bytes: usize) -> InternalBlockProbe {
    let before = esp_alloc::HEAP.free();
    let mut low = 0;
    let mut high = before.saturating_sub(reserve_bytes) / 8 + 1;
    while low + 1 < high {
        let candidate = low + (high - low) / 2;
        let layout = core::alloc::Layout::from_size_align(candidate * 8, 8).unwrap();
        // SAFETY: the exact successful allocation is freed once with its
        // original layout; no references or initialized values escape.
        let passed = unsafe {
            let ptr = core::alloc::GlobalAlloc::alloc(&esp_alloc::HEAP, layout);
            if ptr.is_null() {
                false
            } else {
                let preserved = esp_alloc::HEAP.free() >= reserve_bytes;
                core::alloc::GlobalAlloc::dealloc(&esp_alloc::HEAP, ptr, layout);
                preserved
            }
        };
        if passed {
            low = candidate;
        } else {
            high = candidate;
        }
    }
    let after = esp_alloc::HEAP.free();
    InternalBlockProbe {
        free_before_bytes: before,
        block_bytes: low * 8,
        reserve_bytes,
        free_after_bytes: after,
        stable: before == after,
    }
}
