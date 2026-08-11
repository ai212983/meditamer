use core::sync::atomic::Ordering;

use super::{
    current_allocator_state, maybe_log_new_peak, min_or_zero, update_min_observed,
    update_peak_used_bytes, used_bytes, AllocatorMemorySnapshot, AllocatorState, AllocatorStatus,
    LARGE_ALLOC_EXTERNAL_OK, LARGE_ALLOC_FAIL, LARGE_ALLOC_INTERNAL_OK, MIN_FREE_BYTES,
    MIN_FREE_EXTERNAL_BYTES, MIN_FREE_INTERNAL_BYTES,
};

pub(crate) fn allocator_status() -> AllocatorStatus {
    let (total_bytes, free_bytes) = {
        let stats = esp_alloc::HEAP.stats();
        (stats.size, esp_alloc::HEAP.free())
    };
    let peak_used_bytes = update_peak_used_bytes(used_bytes(total_bytes, free_bytes));

    AllocatorStatus {
        feature_enabled: true,
        state: current_allocator_state(),
        total_bytes,
        free_bytes,
        peak_used_bytes,
    }
}

pub(crate) fn allocator_memory_snapshot() -> AllocatorMemorySnapshot {
    let status = allocator_status();
    let used_bytes = used_bytes(status.total_bytes, status.free_bytes);
    let (free_internal_bytes, free_external_bytes) = (
        esp_alloc::HEAP.free_caps(esp_alloc::MemoryCapability::Internal.into()),
        esp_alloc::HEAP.free_caps(esp_alloc::MemoryCapability::External.into()),
    );

    let initialized = status.feature_enabled && matches!(status.state, AllocatorState::Initialized);
    let (min_free_bytes, min_free_internal_bytes, min_free_external_bytes) = if initialized {
        (
            update_min_observed(&MIN_FREE_BYTES, status.free_bytes),
            update_min_observed(&MIN_FREE_INTERNAL_BYTES, free_internal_bytes),
            update_min_observed(&MIN_FREE_EXTERNAL_BYTES, free_external_bytes),
        )
    } else {
        (
            MIN_FREE_BYTES.load(Ordering::Relaxed),
            MIN_FREE_INTERNAL_BYTES.load(Ordering::Relaxed),
            MIN_FREE_EXTERNAL_BYTES.load(Ordering::Relaxed),
        )
    };

    AllocatorMemorySnapshot {
        feature_enabled: status.feature_enabled,
        state: status.state,
        total_bytes: status.total_bytes,
        used_bytes,
        free_bytes: status.free_bytes,
        peak_used_bytes: status.peak_used_bytes,
        free_internal_bytes,
        free_external_bytes,
        min_free_bytes: min_or_zero(min_free_bytes),
        min_free_internal_bytes: min_or_zero(min_free_internal_bytes),
        min_free_external_bytes: min_or_zero(min_free_external_bytes),
        large_alloc_external_ok: LARGE_ALLOC_EXTERNAL_OK.load(Ordering::Relaxed),
        large_alloc_internal_ok: LARGE_ALLOC_INTERNAL_OK.load(Ordering::Relaxed),
        large_alloc_fail: LARGE_ALLOC_FAIL.load(Ordering::Relaxed),
    }
}

pub(crate) fn log_allocator_status() {
    let snapshot = allocator_memory_snapshot();
    esp_println::println!(
        "psram: feature_enabled={} state={:?} total_bytes={} used_bytes={} free_bytes={} peak_used_bytes={} internal_free_bytes={} external_free_bytes={} min_free_bytes={} min_internal_free_bytes={} min_external_free_bytes={} large_alloc_external_ok={} large_alloc_internal_ok={} large_alloc_fail={}",
        snapshot.feature_enabled,
        snapshot.state,
        snapshot.total_bytes,
        snapshot.used_bytes,
        snapshot.free_bytes,
        snapshot.peak_used_bytes,
        snapshot.free_internal_bytes,
        snapshot.free_external_bytes,
        snapshot.min_free_bytes,
        snapshot.min_free_internal_bytes,
        snapshot.min_free_external_bytes,
        snapshot.large_alloc_external_ok,
        snapshot.large_alloc_internal_ok,
        snapshot.large_alloc_fail
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
