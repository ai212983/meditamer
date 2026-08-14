use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
    sync::atomic::Ordering,
};

use super::provenance::internal_low_water_snapshot;
use super::{
    current_allocator_state, maybe_log_new_peak, min_or_zero, update_min_observed,
    update_peak_used_bytes, used_bytes, AllocatorMemorySnapshot, AllocatorState, AllocatorStatus,
    InternalBlockProbe, LARGE_ALLOC_EXTERNAL_OK, LARGE_ALLOC_FAIL, LARGE_ALLOC_INTERNAL_OK,
    MIN_FREE_BYTES, MIN_FREE_EXTERNAL_BYTES,
};

// linked_list_allocator rounds layouts to at least two machine words. Keep
// every binary-search candidate on that exact granularity so the allocation
// hook cannot observe a successfully rounded probe allocation below the
// preserved reserve.
const INTERNAL_PROBE_ALIGNMENT: usize = 2 * core::mem::size_of::<usize>();

fn try_internal_allocation_preserving(bytes: usize, reserve_bytes: usize) -> bool {
    let Ok(layout) = Layout::from_size_align(bytes, INTERNAL_PROBE_ALIGNMENT) else {
        return false;
    };
    let raw =
        unsafe { esp_alloc::HEAP.alloc_caps(esp_alloc::MemoryCapability::Internal.into(), layout) };
    let Some(ptr) = NonNull::new(raw) else {
        return false;
    };
    let reserve_preserved =
        esp_alloc::HEAP.free_caps(esp_alloc::MemoryCapability::Internal.into()) >= reserve_bytes;
    unsafe {
        GlobalAlloc::dealloc(&esp_alloc::HEAP, ptr.as_ptr(), layout);
    }
    reserve_preserved
}

/// Measures a contiguous allocatable lower bound while preserving `reserve_bytes`.
/// Call only from task context at an explicit diagnostic checkpoint. Pass zero
/// only after every radio/controller callback source is disabled and quiescent.
pub(crate) fn probe_internal_block_above_reserve(reserve_bytes: usize) -> InternalBlockProbe {
    if !matches!(current_allocator_state(), AllocatorState::Initialized) {
        return InternalBlockProbe {
            free_before_bytes: 0,
            block_bytes: 0,
            reserve_bytes,
            free_after_bytes: 0,
            stable: false,
        };
    }

    let internal = esp_alloc::MemoryCapability::Internal.into();
    let free_before_bytes = esp_alloc::HEAP.free_caps(internal);
    let probe_limit_bytes = free_before_bytes.saturating_sub(reserve_bytes);
    let units = probe_limit_bytes / INTERNAL_PROBE_ALIGNMENT;
    let mut passing_units = 0usize;
    let mut failing_units = units.saturating_add(1);
    while passing_units.saturating_add(1) < failing_units {
        let candidate_units = passing_units + (failing_units - passing_units) / 2;
        let candidate_bytes = candidate_units * INTERNAL_PROBE_ALIGNMENT;
        if try_internal_allocation_preserving(candidate_bytes, reserve_bytes) {
            passing_units = candidate_units;
        } else {
            failing_units = candidate_units;
        }
    }
    let free_after_bytes = esp_alloc::HEAP.free_caps(internal);

    InternalBlockProbe {
        free_before_bytes,
        block_bytes: passing_units * INTERNAL_PROBE_ALIGNMENT,
        reserve_bytes,
        free_after_bytes,
        stable: free_before_bytes == free_after_bytes,
    }
}

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
    let internal_low_water = internal_low_water_snapshot();
    let (min_free_bytes, min_free_external_bytes) = if initialized {
        (
            update_min_observed(&MIN_FREE_BYTES, status.free_bytes),
            update_min_observed(&MIN_FREE_EXTERNAL_BYTES, free_external_bytes),
        )
    } else {
        (
            MIN_FREE_BYTES.load(Ordering::Relaxed),
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
        min_free_internal_bytes: internal_low_water.free_bytes,
        min_internal_alloc_charge_bytes: internal_low_water.charge_bytes,
        min_internal_alloc_internal_required: internal_low_water.internal_required,
        min_internal_alloc_charge_overflow: internal_low_water.charge_overflow,
        min_internal_alloc_post_free_bytes: internal_low_water.free_bytes,
        min_internal_alloc_correlation_stable: internal_low_water.correlation_stable,
        min_internal_alloc_wifi_rx_matched: internal_low_water.wifi_rx_matched,
        min_internal_alloc_released: internal_low_water.released,
        min_free_external_bytes: min_or_zero(min_free_external_bytes),
        large_alloc_external_ok: LARGE_ALLOC_EXTERNAL_OK.load(Ordering::Relaxed),
        large_alloc_internal_ok: LARGE_ALLOC_INTERNAL_OK.load(Ordering::Relaxed),
        large_alloc_fail: LARGE_ALLOC_FAIL.load(Ordering::Relaxed),
    }
}

pub(crate) fn log_allocator_status() {
    let snapshot = allocator_memory_snapshot();
    esp_println::println!(
        "psram: feature_enabled={} state={:?} total_bytes={} used_bytes={} free_bytes={} peak_used_bytes={} internal_free_bytes={} external_free_bytes={} min_free_bytes={} min_internal_free_bytes={} min_internal_alloc_charge_bytes={} min_internal_alloc_internal_required={} min_internal_alloc_charge_overflow={} min_internal_alloc_post_free_bytes={} min_internal_alloc_correlation_stable={} min_internal_alloc_wifi_rx_matched={} min_internal_alloc_released={} min_external_free_bytes={} large_alloc_external_ok={} large_alloc_internal_ok={} large_alloc_fail={}",
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
        snapshot.min_internal_alloc_charge_bytes,
        snapshot.min_internal_alloc_internal_required,
        snapshot.min_internal_alloc_charge_overflow,
        snapshot.min_internal_alloc_post_free_bytes,
        snapshot.min_internal_alloc_correlation_stable,
        snapshot.min_internal_alloc_wifi_rx_matched,
        snapshot.min_internal_alloc_released,
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
