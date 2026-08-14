//! Allocation low-water provenance and vendor RX correlation.
//!
//! The allocator hooks in this module preserve the allocation that established
//! the run-wide internal-heap low-water. The vendor Wi-Fi RX hook can then
//! correlate that allocation with a station receive callback without allocating,
//! waiting, logging, or changing packet admission.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use super::{current_allocator_state, AllocatorState};

const INTERNAL_LOW_WATER_FREE_BITS: u32 = 17;
const INTERNAL_LOW_WATER_FREE_MASK: u32 = (1 << INTERNAL_LOW_WATER_FREE_BITS) - 1;
const INTERNAL_LOW_WATER_CHARGE_SHIFT: u32 = INTERNAL_LOW_WATER_FREE_BITS;
const INTERNAL_LOW_WATER_CHARGE_BITS: u32 = 14;
const INTERNAL_LOW_WATER_CHARGE_MASK: u32 = (1 << INTERNAL_LOW_WATER_CHARGE_BITS) - 1;
const INTERNAL_LOW_WATER_INTERNAL_REQUIRED_SHIFT: u32 = 31;
const INTERNAL_LOW_WATER_CHARGE_QUANTUM: usize = core::mem::align_of::<usize>();
const INTERNAL_LOW_WATER_SENTINEL: u32 = INTERNAL_LOW_WATER_FREE_MASK;
const _: () = assert!(super::INTERNAL_HEAP_DRAM2_BYTES < INTERNAL_LOW_WATER_SENTINEL as usize);

static INTERNAL_LOW_WATER: AtomicU32 = AtomicU32::new(INTERNAL_LOW_WATER_SENTINEL);
// Generation zero invalidates the multiword correlation descriptor while a
// winning allocation hook publishes it. Match/release words retain that exact
// nonzero generation so stale pointer reuse can only make evidence inconclusive.
static INTERNAL_LOW_WATER_CORRELATION_WRITER: AtomicBool = AtomicBool::new(false);
static INTERNAL_LOW_WATER_CORRELATION_CONFLICT: AtomicBool = AtomicBool::new(false);
static INTERNAL_LOW_WATER_CORRELATION_NEXT_GENERATION: AtomicU32 = AtomicU32::new(1);
static INTERNAL_LOW_WATER_CORRELATION_GENERATION: AtomicU32 = AtomicU32::new(0);
static INTERNAL_LOW_WATER_CORRELATION_PACKED: AtomicU32 =
    AtomicU32::new(INTERNAL_LOW_WATER_SENTINEL);
static INTERNAL_LOW_WATER_CORRELATION_PTR: AtomicUsize = AtomicUsize::new(0);
static INTERNAL_LOW_WATER_CORRELATION_SIZE: AtomicUsize = AtomicUsize::new(0);
static INTERNAL_LOW_WATER_WIFI_RX_MATCH_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static INTERNAL_LOW_WATER_RELEASE_SEQUENCE: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InternalLowWaterSnapshot {
    pub(super) free_bytes: usize,
    pub(super) charge_bytes: usize,
    pub(super) internal_required: bool,
    pub(super) charge_overflow: bool,
    pub(super) correlation_stable: bool,
    pub(super) wifi_rx_matched: bool,
    pub(super) released: bool,
}

/// Record allocator low-water immediately after every successful allocation.
///
/// The reviewed `esp-alloc` patch measures `post_internal_free` while its heap
/// lock still serializes all regions, then invokes this hook after releasing
/// the lock. The single packed atomic therefore keeps the minimum, exact
/// allocator charge, and requested capability class from the same allocation even
/// when both ESP32 cores allocate concurrently.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn _esp_alloc_alloc(
    _heap: &esp_alloc::EspHeap,
    capabilities: esp_alloc::export::enumset::EnumSet<esp_alloc::MemoryCapability>,
    ptr: usize,
    size: usize,
    internal_free_before: usize,
    post_internal_free: usize,
) {
    if ptr == 0 || !matches!(current_allocator_state(), AllocatorState::Initialized) {
        return;
    }
    let internal_charge = internal_free_before.saturating_sub(post_internal_free);
    if internal_charge == 0 {
        return;
    }
    record_internal_low_water(
        post_internal_free,
        internal_charge,
        allocation_requires_internal(capabilities),
        ptr,
        size,
    );
}

#[unsafe(no_mangle)]
unsafe extern "Rust" fn _esp_alloc_dealloc_completed(
    _heap: &esp_alloc::EspHeap,
    ptr: usize,
    size: usize,
) {
    mark_internal_low_water_released(ptr, size);
}

/// Correlate the allocator event that established the run-wide low-water with
/// the vendor station-RX callback boundary.
///
/// `esp-radio` invokes this before it constructs first-party `PacketBuffer`
/// telemetry. The operation is deliberately atomic-only: it cannot allocate,
/// wait, log, or change packet admission. Pointer reuse is rejected after the
/// allocator's deallocation hook marks the winning generation released.
#[unsafe(no_mangle)]
unsafe extern "C" fn _meditamer_match_internal_low_water_wifi_rx(
    buffer: usize,
    len: usize,
    eb: usize,
) {
    match_internal_low_water_wifi_rx(buffer, len, eb);
}

fn allocation_requires_internal(
    capabilities: esp_alloc::export::enumset::EnumSet<esp_alloc::MemoryCapability>,
) -> bool {
    capabilities.contains(esp_alloc::MemoryCapability::Internal)
}

fn pack_internal_low_water(free_bytes: usize, charge_bytes: usize, internal_required: bool) -> u32 {
    let free = free_bytes.min((INTERNAL_LOW_WATER_FREE_MASK - 1) as usize) as u32;
    let charge_units = charge_bytes.saturating_add(INTERNAL_LOW_WATER_CHARGE_QUANTUM - 1)
        / INTERNAL_LOW_WATER_CHARGE_QUANTUM;
    let charge_units = charge_units.min(INTERNAL_LOW_WATER_CHARGE_MASK as usize) as u32;
    free | (charge_units << INTERNAL_LOW_WATER_CHARGE_SHIFT)
        | (u32::from(internal_required) << INTERNAL_LOW_WATER_INTERNAL_REQUIRED_SHIFT)
}

fn unpack_internal_low_water(packed: u32) -> InternalLowWaterSnapshot {
    let free = packed & INTERNAL_LOW_WATER_FREE_MASK;
    if free == INTERNAL_LOW_WATER_SENTINEL {
        return InternalLowWaterSnapshot {
            free_bytes: 0,
            charge_bytes: 0,
            internal_required: true,
            charge_overflow: false,
            correlation_stable: false,
            wifi_rx_matched: false,
            released: false,
        };
    }
    let charge_units = (packed >> INTERNAL_LOW_WATER_CHARGE_SHIFT) & INTERNAL_LOW_WATER_CHARGE_MASK;
    InternalLowWaterSnapshot {
        free_bytes: free as usize,
        charge_bytes: charge_units as usize * INTERNAL_LOW_WATER_CHARGE_QUANTUM,
        internal_required: packed >> INTERNAL_LOW_WATER_INTERNAL_REQUIRED_SHIFT != 0,
        charge_overflow: charge_units == INTERNAL_LOW_WATER_CHARGE_MASK,
        correlation_stable: false,
        wifi_rx_matched: false,
        released: false,
    }
}

fn packed_low_water_is_lower(candidate_free_bytes: usize, current: u32) -> bool {
    current == INTERNAL_LOW_WATER_SENTINEL
        || candidate_free_bytes < unpack_internal_low_water(current).free_bytes
}

fn record_internal_low_water_packed(
    free_bytes: usize,
    charge_bytes: usize,
    internal_required: bool,
) -> bool {
    let candidate = pack_internal_low_water(free_bytes, charge_bytes, internal_required);
    let mut current = INTERNAL_LOW_WATER.load(Ordering::Relaxed);
    while packed_low_water_is_lower(free_bytes, current) {
        match INTERNAL_LOW_WATER.compare_exchange_weak(
            current,
            candidate,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
    false
}

fn record_internal_low_water(
    free_bytes: usize,
    charge_bytes: usize,
    internal_required: bool,
    ptr: usize,
    size: usize,
) {
    let observed = INTERNAL_LOW_WATER.load(Ordering::Relaxed);
    if !packed_low_water_is_lower(free_bytes, observed) {
        return;
    }

    if INTERNAL_LOW_WATER_CORRELATION_WRITER
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        // Never spin in an allocator hook. Preserve the authoritative minimum,
        // but permanently fail correlation closed if this candidate actually
        // wins while another winner is publishing its metadata.
        if record_internal_low_water_packed(free_bytes, charge_bytes, internal_required) {
            INTERNAL_LOW_WATER_CORRELATION_CONFLICT.store(true, Ordering::Release);
        }
        return;
    }

    let candidate = pack_internal_low_water(free_bytes, charge_bytes, internal_required);
    let current = INTERNAL_LOW_WATER.load(Ordering::Relaxed);
    if packed_low_water_is_lower(free_bytes, current) {
        let generation = INTERNAL_LOW_WATER_CORRELATION_NEXT_GENERATION
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        if generation == 0 || ptr.checked_add(size).is_none() {
            INTERNAL_LOW_WATER_CORRELATION_CONFLICT.store(true, Ordering::Release);
            record_internal_low_water_packed(free_bytes, charge_bytes, internal_required);
            INTERNAL_LOW_WATER_CORRELATION_WRITER.store(false, Ordering::Release);
            return;
        }
        // Generation zero invalidates readers while the multiword descriptor
        // is updated. The authoritative packed minimum is committed before the
        // final generation publication, and every reader binds them together.
        INTERNAL_LOW_WATER_CORRELATION_GENERATION.store(0, Ordering::Release);
        if INTERNAL_LOW_WATER
            .compare_exchange(current, candidate, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            INTERNAL_LOW_WATER_CORRELATION_PTR.store(ptr, Ordering::Relaxed);
            INTERNAL_LOW_WATER_CORRELATION_SIZE.store(size, Ordering::Relaxed);
            INTERNAL_LOW_WATER_CORRELATION_PACKED.store(candidate, Ordering::Relaxed);
            INTERNAL_LOW_WATER_WIFI_RX_MATCH_SEQUENCE.store(0, Ordering::Relaxed);
            INTERNAL_LOW_WATER_RELEASE_SEQUENCE.store(0, Ordering::Relaxed);
            INTERNAL_LOW_WATER_CORRELATION_GENERATION.store(generation, Ordering::Release);
        } else {
            INTERNAL_LOW_WATER_CORRELATION_CONFLICT.store(true, Ordering::Release);
        }
    }
    INTERNAL_LOW_WATER_CORRELATION_WRITER.store(false, Ordering::Release);
}

fn range_in_winning_allocation(
    candidate: usize,
    candidate_size: usize,
    ptr: usize,
    size: usize,
) -> bool {
    let Some(candidate_end) = candidate.checked_add(candidate_size) else {
        return false;
    };
    let Some(allocation_end) = ptr.checked_add(size) else {
        return false;
    };
    candidate_size != 0 && candidate >= ptr && candidate_end <= allocation_end
}

fn match_internal_low_water_wifi_rx(buffer: usize, len: usize, eb: usize) {
    if INTERNAL_LOW_WATER_CORRELATION_CONFLICT.load(Ordering::Acquire) {
        return;
    }
    let generation = INTERNAL_LOW_WATER_CORRELATION_GENERATION.load(Ordering::Acquire);
    if generation == 0 {
        return;
    }
    let ptr = INTERNAL_LOW_WATER_CORRELATION_PTR.load(Ordering::Relaxed);
    let size = INTERNAL_LOW_WATER_CORRELATION_SIZE.load(Ordering::Relaxed);
    let packed = INTERNAL_LOW_WATER_CORRELATION_PACKED.load(Ordering::Relaxed);
    if generation != INTERNAL_LOW_WATER_CORRELATION_GENERATION.load(Ordering::Acquire)
        || packed != INTERNAL_LOW_WATER.load(Ordering::Relaxed)
        || INTERNAL_LOW_WATER_RELEASE_SEQUENCE.load(Ordering::Acquire) == generation
        || !range_in_winning_allocation(eb, 1, ptr, size)
        || !range_in_winning_allocation(buffer, len, ptr, size)
    {
        return;
    }
    // A stale callback may overwrite a newer zero with its old generation, but
    // the reporter compares against the current generation and therefore turns
    // that race into a false negative, never a false match.
    publish_current_generation(
        &INTERNAL_LOW_WATER_WIFI_RX_MATCH_SEQUENCE,
        generation,
        packed,
    );
}

fn publish_current_generation(target: &AtomicU32, generation: u32, packed: u32) {
    let mut observed = target.load(Ordering::Acquire);
    loop {
        if observed == generation {
            return;
        }
        if generation != INTERNAL_LOW_WATER_CORRELATION_GENERATION.load(Ordering::Acquire)
            || packed != INTERNAL_LOW_WATER.load(Ordering::Relaxed)
        {
            return;
        }
        match target.compare_exchange_weak(
            observed,
            generation,
            Ordering::Release,
            Ordering::Acquire,
        ) {
            Ok(_) => return,
            Err(current) => observed = current,
        }
    }
}

fn mark_internal_low_water_released(ptr: usize, size: usize) {
    if INTERNAL_LOW_WATER_CORRELATION_CONFLICT.load(Ordering::Acquire) {
        return;
    }
    let generation = INTERNAL_LOW_WATER_CORRELATION_GENERATION.load(Ordering::Acquire);
    if generation == 0 {
        return;
    }
    let winning_ptr = INTERNAL_LOW_WATER_CORRELATION_PTR.load(Ordering::Relaxed);
    let packed = INTERNAL_LOW_WATER_CORRELATION_PACKED.load(Ordering::Relaxed);
    let winning_size = INTERNAL_LOW_WATER_CORRELATION_SIZE.load(Ordering::Relaxed);
    if ptr == winning_ptr
        && size == winning_size
        && generation == INTERNAL_LOW_WATER_CORRELATION_GENERATION.load(Ordering::Acquire)
        && packed == INTERNAL_LOW_WATER.load(Ordering::Relaxed)
    {
        publish_current_generation(&INTERNAL_LOW_WATER_RELEASE_SEQUENCE, generation, packed);
    }
}

pub(super) fn seed_internal_low_water(free_bytes: usize) {
    let packed = pack_internal_low_water(free_bytes, 0, true);
    INTERNAL_LOW_WATER.store(packed, Ordering::Relaxed);
    INTERNAL_LOW_WATER_CORRELATION_PTR.store(0, Ordering::Relaxed);
    INTERNAL_LOW_WATER_CORRELATION_SIZE.store(0, Ordering::Relaxed);
    INTERNAL_LOW_WATER_CORRELATION_PACKED.store(packed, Ordering::Relaxed);
    INTERNAL_LOW_WATER_WIFI_RX_MATCH_SEQUENCE.store(0, Ordering::Relaxed);
    INTERNAL_LOW_WATER_RELEASE_SEQUENCE.store(0, Ordering::Relaxed);
    INTERNAL_LOW_WATER_CORRELATION_WRITER.store(false, Ordering::Relaxed);
    INTERNAL_LOW_WATER_CORRELATION_CONFLICT.store(false, Ordering::Relaxed);
    INTERNAL_LOW_WATER_CORRELATION_NEXT_GENERATION.store(1, Ordering::Relaxed);
    INTERNAL_LOW_WATER_CORRELATION_GENERATION.store(0, Ordering::Release);
}

pub(super) fn internal_low_water_snapshot() -> InternalLowWaterSnapshot {
    let generation = INTERNAL_LOW_WATER_CORRELATION_GENERATION.load(Ordering::Acquire);
    let packed = INTERNAL_LOW_WATER.load(Ordering::Relaxed);
    let correlation_packed = INTERNAL_LOW_WATER_CORRELATION_PACKED.load(Ordering::Relaxed);
    let matched_sequence = INTERNAL_LOW_WATER_WIFI_RX_MATCH_SEQUENCE.load(Ordering::Acquire);
    let released_sequence = INTERNAL_LOW_WATER_RELEASE_SEQUENCE.load(Ordering::Acquire);
    let generation_after = INTERNAL_LOW_WATER_CORRELATION_GENERATION.load(Ordering::Acquire);
    let mut snapshot = unpack_internal_low_water(packed);
    snapshot.correlation_stable = generation != 0
        && generation == generation_after
        && correlation_packed == packed
        && !INTERNAL_LOW_WATER_CORRELATION_CONFLICT.load(Ordering::Acquire);
    snapshot.wifi_rx_matched = snapshot.correlation_stable && matched_sequence == generation;
    snapshot.released = snapshot.correlation_stable && released_sequence == generation;
    snapshot
}
