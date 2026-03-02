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
    MIN_FREE_BYTES.store(usize::MAX, Ordering::Relaxed);
    MIN_FREE_INTERNAL_BYTES.store(usize::MAX, Ordering::Relaxed);
    MIN_FREE_EXTERNAL_BYTES.store(usize::MAX, Ordering::Relaxed);
    LARGE_ALLOC_EXTERNAL_OK.store(0, Ordering::Relaxed);
    LARGE_ALLOC_INTERNAL_OK.store(0, Ordering::Relaxed);
    LARGE_ALLOC_FAIL.store(0, Ordering::Relaxed);
    update_allocator_state(AllocatorState::Initialized);
    allocator_status()
}

#[cfg(not(feature = "psram-alloc"))]
pub(crate) fn init_allocator() -> AllocatorStatus {
    allocator_status()
}
