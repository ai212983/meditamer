use core::sync::atomic::Ordering;

use super::{
    allocator_status, current_allocator_state, update_allocator_state, AllocatorState,
    LARGE_ALLOC_EXTERNAL_OK, LARGE_ALLOC_FAIL, LARGE_ALLOC_INTERNAL_OK,
    LAST_LOGGED_PEAK_USED_BYTES, MIN_FREE_BYTES, MIN_FREE_EXTERNAL_BYTES, MIN_FREE_INTERNAL_BYTES,
    PEAK_USED_BYTES,
};
use super::{AllocatorStatus, INTERNAL_HEAP_DRAM2_BYTES};

pub(crate) fn init_allocator(psram: esp_hal::peripherals::PSRAM<'static>) -> AllocatorStatus {
    if matches!(current_allocator_state(), AllocatorState::Initialized) {
        return allocator_status();
    }

    // Internal-capability heap for subsystems (Wi-Fi) that cannot allocate from
    // external PSRAM. It sits outside `dram_seg`, so none of it comes out of the
    // CPU0 stack.
    esp_alloc::heap_allocator!(
        #[unsafe(link_section = ".dram2_uninit")]
        size: INTERNAL_HEAP_DRAM2_BYTES
    );

    let config = esp_hal::psram::PsramConfig {
        cache_speed: esp_hal::psram::PsramCacheSpeed::PsramCacheF80mS80m,
        ..Default::default()
    };
    let psram = esp_hal::psram::Psram::new(psram, config);
    let (_start, size) = psram.raw_parts();
    if size == 0 {
        update_allocator_state(AllocatorState::InitFailed);
        return allocator_status();
    }

    esp_alloc::psram_allocator!(&psram);
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
