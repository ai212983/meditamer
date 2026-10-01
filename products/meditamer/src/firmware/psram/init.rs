use core::sync::atomic::Ordering;

use super::provenance::seed_internal_low_water;
#[cfg(feature = "classic-bt-memory-reclaim")]
use super::CLASSIC_BT_HEAP_BYTES;
use super::{
    allocator_status, current_allocator_state, update_allocator_state, AllocatorState,
    LARGE_ALLOC_EXTERNAL_OK, LARGE_ALLOC_FAIL, LARGE_ALLOC_INTERNAL_OK,
    LAST_LOGGED_PEAK_USED_BYTES, MIN_FREE_BYTES, MIN_FREE_EXTERNAL_BYTES, PEAK_USED_BYTES,
};
use super::{AllocatorStatus, INTERNAL_HEAP_DRAM2_BYTES};

#[cfg(feature = "classic-bt-memory-reclaim")]
const _: () = assert!(CLASSIC_BT_HEAP_BYTES == esp_radio::ble::CLASSIC_BT_MEMORY_SIZE);

pub fn init_allocator(psram: esp_hal::peripherals::PSRAM<'static>) -> AllocatorStatus {
    if matches!(current_allocator_state(), AllocatorState::Initialized) {
        return allocator_status();
    }

    // Internal-capability heap for subsystems (Wi-Fi) that cannot allocate from
    // external PSRAM. It sits outside `dram_seg`, so none of it comes out of the
    // CPU0 stack.
    esp_alloc::heap_allocator!(
        #[unsafe(link_section = ".dram2_uninit.heap")]
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

    #[cfg(feature = "classic-bt-memory-reclaim")]
    let classic_bt_memory = esp_radio::ble::release_classic_bt_memory()
        .unwrap_or_else(|error| panic!("failed to release Classic Bluetooth memory: {error:?}"));

    #[cfg(feature = "classic-bt-memory-reclaim")]
    {
        let (start, size) = classic_bt_memory.into_raw_parts();
        assert_eq!(size, CLASSIC_BT_HEAP_BYTES);
        unsafe {
            esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
                start,
                size,
                esp_alloc::MemoryCapability::Internal.into(),
            ));
        }
    }

    esp_alloc::psram_allocator!(&psram);
    PEAK_USED_BYTES.store(0, Ordering::Relaxed);
    LAST_LOGGED_PEAK_USED_BYTES.store(0, Ordering::Relaxed);
    MIN_FREE_BYTES.store(usize::MAX, Ordering::Relaxed);
    seed_internal_low_water(
        esp_alloc::HEAP.free_caps(esp_alloc::MemoryCapability::Internal.into()),
    );
    MIN_FREE_EXTERNAL_BYTES.store(usize::MAX, Ordering::Relaxed);
    LARGE_ALLOC_EXTERNAL_OK.store(0, Ordering::Relaxed);
    LARGE_ALLOC_INTERNAL_OK.store(0, Ordering::Relaxed);
    LARGE_ALLOC_FAIL.store(0, Ordering::Relaxed);
    update_allocator_state(AllocatorState::Initialized);
    allocator_status()
}
