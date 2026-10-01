//! Physical, all-or-nothing backing for an ordered bundle.
//!
//! The caller supplies one capability-specific allocator. Every part and the
//! small metadata vector use that allocator; this module never falls back to
//! another memory class. Parts are separate aligned allocations, matching
//! callers that need contiguous access to each part but not one giant region.
//! A successful prepare owns every physical byte before arena commit. This is
//! a cooperative reservation, not a promise that an uncoordinated shared
//! heap will have enough space for the next request.

use allocator_api2::alloc::{Allocator, Layout};
use allocator_api2::vec::Vec;
use core::ptr::NonNull;
use core::slice;
use residency_policy::{Bundle, BundleError, BundleId};

use crate::{BackingToken, BundleAdmissionError, BundleReservation, PhaseArena, Span};

/// Why a complete physical bundle could not be acquired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackingError {
    InvalidDeclaration(BundleError),
    WrongPartCount,
    MetadataAllocationFailed,
    InvalidPartLayout { index: usize },
    PartAllocationFailed { index: usize },
}

/// Which step of joint structural and physical preparation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareBackingError {
    Arena(BundleAdmissionError),
    Backing(BackingError),
}

/// One owned allocation made with the caller's capability allocator.
struct Part {
    ptr: NonNull<u8>,
    layout: Layout,
}

// SAFETY: Part uniquely owns its allocated byte range. Moving ownership to
// another thread moves the sole release responsibility with it.
unsafe impl Send for Part {}
// SAFETY: shared access exposes only immutable bytes; mutable access requires
// an exclusive borrow of the enclosing BundleBacking.
unsafe impl Sync for Part {}

/// Physical bytes for one named bundle. The metadata and every part use `A`.
/// Dropping this value deallocates all parts, including after partial failure.
pub struct BundleBacking<A: Allocator> {
    id: BundleId,
    version: u16,
    parts: Vec<Part, A>,
}

impl<A: Allocator> BundleBacking<A> {
    /// Acquire every part in declaration order with exact requested alignment.
    /// Any failure frees all preceding parts before returning.
    pub fn try_new_zeroed(
        bundle: &Bundle<'_>,
        max_align: usize,
        alloc: A,
    ) -> Result<Self, BackingError> {
        bundle
            .layout(max_align)
            .map_err(BackingError::InvalidDeclaration)?;
        let mut backing = Self {
            id: bundle.id,
            version: bundle.version,
            parts: Vec::new_in(alloc),
        };
        backing
            .parts
            .try_reserve_exact(bundle.parts.len())
            .map_err(|_| BackingError::MetadataAllocationFailed)?;
        for (index, part) in bundle.parts.iter().enumerate() {
            let layout = Layout::from_size_align(part.bytes, part.align)
                .map_err(|_| BackingError::InvalidPartLayout { index })?;
            let memory = backing
                .parts
                .allocator()
                .allocate_zeroed(layout)
                .map_err(|_| BackingError::PartAllocationFailed { index })?;
            backing.parts.push(Part {
                ptr: memory.cast(),
                layout,
            });
        }
        Ok(backing)
    }

    pub const fn id(&self) -> BundleId {
        self.id
    }

    pub const fn version(&self) -> u16 {
        self.version
    }

    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// Borrow one physically contiguous part. Out-of-range indices are `None`.
    pub fn part(&self, index: usize) -> Option<&[u8]> {
        let part = self.parts.get(index)?;
        // SAFETY: the part owns a live allocation of exactly this layout;
        // shared borrowing forbids overlapping mutable access.
        Some(unsafe { slice::from_raw_parts(part.ptr.as_ptr(), part.layout.size()) })
    }

    /// Mutably borrow one physically contiguous part.
    pub fn part_mut(&mut self, index: usize) -> Option<&mut [u8]> {
        let part = self.parts.get_mut(index)?;
        // SAFETY: the part owns a live allocation of exactly this layout;
        // &mut self excludes all overlapping borrows.
        Some(unsafe { slice::from_raw_parts_mut(part.ptr.as_ptr(), part.layout.size()) })
    }
}

impl<A: Allocator> Drop for BundleBacking<A> {
    fn drop(&mut self) {
        for part in self.parts.iter() {
            // SAFETY: every Part was allocated through this exact allocator
            // with this layout and is deallocated exactly once here.
            unsafe { self.parts.allocator().deallocate(part.ptr, part.layout) };
        }
    }
}

/// Structural reservation plus physically acquired bytes before publication.
/// Dropping this value rolls both acquisitions back.
pub struct PreparedBacking<'a, A: Allocator, const N: usize> {
    reservation: BundleReservation<'a, N>,
    backing: BundleBacking<A>,
}

impl<'a, A: Allocator, const N: usize> PreparedBacking<'a, A, N> {
    pub const fn spans(&self) -> &[Span; N] {
        self.reservation.spans()
    }

    pub fn part_mut(&mut self, index: usize) -> Option<&mut [u8]> {
        self.backing.part_mut(index)
    }

    /// Publish only after the caller has loaded and validated every required
    /// part. The committed bundle keeps the arena exclusively borrowed until
    /// it is dropped, then frees physical bytes before resetting the arena.
    pub fn commit(mut self) -> BackedBundle<'a, A, N> {
        let token = self.reservation.commit_in_place();
        BackedBundle {
            reservation: self.reservation,
            token: Some(token),
            backing: Some(self.backing),
        }
    }
}

/// Stage structural capacity, then acquire all physical parts. No arena
/// ownership survives any allocation or declaration failure.
pub fn prepare_backed_bundle<'a, A: Allocator, const N: usize>(
    arena: &'a mut PhaseArena,
    bundle: &Bundle<'_>,
    alloc: A,
) -> Result<PreparedBacking<'a, A, N>, PrepareBackingError> {
    let max_align = arena.max_align();
    let reservation = arena
        .reserve_bundle::<N>(bundle)
        .map_err(PrepareBackingError::Arena)?;
    let backing = BundleBacking::try_new_zeroed(bundle, max_align, alloc)
        .map_err(PrepareBackingError::Backing)?;
    Ok(PreparedBacking {
        reservation,
        backing,
    })
}

/// Committed physical bytes and their exclusive arena lease. Dropping this
/// value frees every part and resets the arena; there is no forgotten-token
/// path and no caller-supplied arena to mismatch on release.
pub struct BackedBundle<'a, A: Allocator, const N: usize> {
    reservation: BundleReservation<'a, N>,
    token: Option<BackingToken>,
    backing: Option<BundleBacking<A>>,
}

impl<A: Allocator, const N: usize> BackedBundle<'_, A, N> {
    pub const fn spans(&self) -> &[Span; N] {
        self.reservation.spans()
    }

    pub fn part(&self, index: usize) -> Option<&[u8]> {
        self.backing.as_ref()?.part(index)
    }

    pub fn part_mut(&mut self, index: usize) -> Option<&mut [u8]> {
        self.backing.as_mut()?.part_mut(index)
    }

    /// Explicitly end the phase; dropping has the same effect.
    pub fn release(self) {
        drop(self);
    }
}

impl<A: Allocator, const N: usize> Drop for BackedBundle<'_, A, N> {
    fn drop(&mut self) {
        drop(self.backing.take());
        if let Some(token) = self.token.take() {
            self.reservation
                .release_committed(token)
                .expect("backed bundle owns the only live arena token");
        }
    }
}
