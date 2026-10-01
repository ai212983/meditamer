//! Generic PSRAM placement adapter for any product-owned allocation set.
//!
//! The platform mechanism receives only the strict external capability. It
//! never falls back to internal RAM; no screen name or allocation size lives
//! here. A successful prepare owns every requested physical part already.

use phase_arena::{
    prepare_backed_bundle, PartRequest, PhaseArena, PrepareBackingError, PreparedBacking,
    RuntimeBundle, RuntimeBundleError,
};
use residency_policy::Bundle;

use super::{current_allocator_state, AllocatorState};

/// A PSRAM bundle cannot be prepared before the capability heap exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalBundleError {
    AllocatorNotReady,
    Prepare(PrepareBackingError),
}

/// Acquire all declared parts from PSRAM as one rollback-safe phase.
///
/// This is an actual reservation on success, not a free-byte probe. It does
/// not promise that a later request to the shared heap will succeed; a hard
/// capacity guarantee needs target-owned disjoint/cooperatively leased space.
pub fn prepare_external_bundle<'a, const N: usize>(
    arena: &'a mut PhaseArena,
    bundle: &Bundle<'_>,
) -> Result<PreparedBacking<'a, esp_alloc::ExternalMemory, N>, ExternalBundleError> {
    if !matches!(current_allocator_state(), AllocatorState::Initialized) {
        return Err(ExternalBundleError::AllocatorNotReady);
    }
    prepare_backed_bundle(arena, bundle, esp_alloc::ExternalMemory)
        .map_err(ExternalBundleError::Prepare)
}

pub const MAX_RUNTIME_BUNDLE_BYTES: usize = 3 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeBundleAcquireError {
    AllocatorNotReady,
    InvalidRequest(RuntimeBundleError),
    AllocationFailed(RuntimeBundleError),
}

pub fn acquire_runtime_bundle(
    requests: &[PartRequest],
) -> Result<RuntimeBundle<esp_alloc::ExternalMemory>, RuntimeBundleAcquireError> {
    if !matches!(current_allocator_state(), AllocatorState::Initialized) {
        return Err(RuntimeBundleAcquireError::AllocatorNotReady);
    }
    RuntimeBundle::try_new_zeroed(
        requests,
        MAX_RUNTIME_BUNDLE_BYTES,
        esp_alloc::ExternalMemory,
    )
    .map_err(|error| match error {
        RuntimeBundleError::MetadataAllocationFailed
        | RuntimeBundleError::PartAllocationFailed { .. } => {
            RuntimeBundleAcquireError::AllocationFailed(error)
        }
        _ => RuntimeBundleAcquireError::InvalidRequest(error),
    })
}
